use crate::backend::{apply_event, BackendCommand, BackendPort};
use crate::model::{AppState, ConnectionState, SessionView, TurnView};
use crate::perf::PerfCounters;
use crate::theme::{self, Tokens};
use egui::{
    Align, CentralPanel, Color32, Context, Frame, Id, Layout, Margin, Order, RichText, ScrollArea,
    Sense, SidePanel, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

const SIDEBAR_WIDTH: f32 = 240.0;
const SIDEBAR_MIN_WIDTH: f32 = 170.0;
const SIDEBAR_MAX_WIDTH: f32 = 420.0;
const COMPACT_SIDEBAR_BREAKPOINT: f32 = 1024.0;
const DEFAULT_SIDEBAR_SPLIT: f32 = 0.60;
const CHAT_COLUMN_WIDTH: f32 = 1000.0;
/// Height of the draggable band between the workspace and directory panes.
const SIDEBAR_SPLITTER_HEIGHT: f32 = 9.0;
/// Minimum width of a working-directory context menu, matching the shell.
const WORKSPACE_MENU_MIN_WIDTH: f32 = 148.0;
/// Minimum width of the thread context menu (rename/trajectory/archive/multi).
const THREAD_MENU_MIN_WIDTH: f32 = 148.0;
/// Height of the account row pinned to the bottom of the sidebar.
const SIDEBAR_FOOTER_HEIGHT: f32 = 33.0;

/// Attachment chip geometry, mirrored from the Slint composer: one 220×28 chip
/// plus the 6px gap. Chips wrap instead of overflowing, because eight chips at
/// the minimum window width do not fit on one row.
const COMPOSER_CHIP_WIDTH: f32 = 220.0;
const COMPOSER_CHIP_HEIGHT: f32 = 28.0;
const COMPOSER_CHIP_GAP: f32 = 6.0;
const COMPOSER_CHIP_ROW_HEIGHT: f32 = 34.0;
/// One queued prompt row: a badge, one elided line and a remove button.
const COMPOSER_QUEUE_ROW_HEIGHT: f32 = 26.0;
/// How long a composer notice ("已加入待发送队列") stays on screen. Long enough
/// to read once, short enough that it never looks like persistent state.
const COMPOSER_NOTICE_SECONDS: f64 = 4.0;
/// Upper bound applied to one frame's drained events. The queue itself stays
/// bounded by the worker; this only keeps a single paint from doing unbounded
/// reducer work.
const EVENT_DRAIN_BUDGET: usize = 256;
/// A silent turn still needs a frame once a second so the elapsed-time footer
/// advances. All real content frames arrive through the backend waker.
const IDLE_CLOCK_REPAINT: Duration = Duration::from_millis(1_000);

/// Repaint sources other than a backend wake. `None` means the frame loop may
/// sleep until input or the next wake; nothing here runs while idle and ready.
fn scheduled_frame_delay(
    connection: ConnectionState,
    busy: bool,
    drain_saturated: bool,
) -> Option<Duration> {
    if drain_saturated {
        Some(Duration::ZERO)
    } else if connection == ConnectionState::Starting || busy {
        Some(IDLE_CLOCK_REPAINT)
    } else {
        None
    }
}

/// How many chip rows fit in `width`.
///
/// The composer asks this twice per frame (once to reserve height, once to
/// lay the chips out), so it stays a pure function of the count and width.
fn composer_chip_rows(count: usize, width: f32) -> usize {
    if count == 0 {
        return 0;
    }
    let per_row = ((width + COMPOSER_CHIP_GAP) / (COMPOSER_CHIP_WIDTH + COMPOSER_CHIP_GAP))
        .floor()
        .max(1.0) as usize;
    count.div_ceil(per_row)
}

/// Height the strip above the input card needs for chips and queued prompts.
fn composer_strip_height(attachments: usize, queued: usize, width: f32) -> f32 {
    let chips = composer_chip_rows(attachments, width) as f32 * COMPOSER_CHIP_ROW_HEIGHT;
    let queue = queued as f32 * COMPOSER_QUEUE_ROW_HEIGHT;
    if chips + queue > 0.0 {
        chips + queue + COMPOSER_CHIP_GAP
    } else {
        0.0
    }
}

fn uses_compact_sidebar(screen_width: f32) -> bool {
    screen_width < COMPACT_SIDEBAR_BREAKPOINT
}

fn drawer_sidebar_width(screen_width: f32) -> f32 {
    // Always keep a 30px strip of the chat canvas visible as the dismissal
    // target, even when the viewport is narrower than the minimum sidebar:
    // covering the whole viewport would leave no way to close the drawer by
    // pointer. The drawer never exceeds the viewport either.
    let available = (screen_width - 30.0).max(1.0);
    available
        .min(SIDEBAR_MAX_WIDTH)
        .max(SIDEBAR_MIN_WIDTH.min(available))
        .min(screen_width.max(1.0))
}

fn clamp_sidebar_split(value: f32) -> f32 {
    value.clamp(0.30, 0.80)
}

/// Workspace share of the sidebar scroll track for a pointer at `pointer_y`.
///
/// The splitter must be resolved from the screen pointer position rather than
/// the widget rect: the rect moves as the split changes, and deriving the new
/// split from it makes the handle chase and jitter under the cursor.
fn split_from_pointer(pointer_y: f32, track_top: f32, track_height: f32) -> f32 {
    let usable = (track_height - SIDEBAR_SPLITTER_HEIGHT).max(1.0);
    clamp_sidebar_split((pointer_y - track_top) / usable)
}

fn valid_thread_title(title: &str) -> bool {
    let title = title.trim();
    !title.is_empty() && title.chars().count() <= 120 && !title.chars().any(char::is_control)
}

fn valid_workspace_entry_path(path: &str) -> bool {
    let path = path.trim();
    !path.is_empty()
        && !path.chars().any(char::is_control)
        && !std::path::Path::new(path).is_absolute()
        && path
            .replace('\\', "/")
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// A rename target is a single path segment: the façade renames inside the
/// entry's own parent, so separators must be rejected before submitting.
fn valid_workspace_entry_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 120
        && !name.chars().any(char::is_control)
        && !name.contains('/')
        && !name.contains('\\')
        && name != "."
        && name != ".."
}

/// Parent directory of a workspace-relative path. The workspace root itself
/// has no parent, so the caller can disable "up" without guessing.
enum ThreadRowAction {
    Select(String),
    Rename { id: String, title: String },
    Trajectory { id: String, title: String },
    Archive(String),
    BeginMultiSelect,
    ToggleSelection(String),
}

/// One bounded process-page read requested while painting the transcript.
/// Collecting these instead of mutating `state.turns` mid-loop keeps drawing
/// on a borrowed projection and removes the per-frame transcript clone.
struct ProcessPageRequest {
    turn_id: String,
    before: Option<i64>,
    after: Option<i64>,
}

/// Deferred intent from the device → workspace → thread tree. Returning an
/// intent keeps the scroll closure free of `&mut self` calls, so painting the
/// rail cannot re-enter the state it is reading.
enum SidebarRowAction {
    Thread(ThreadRowAction),
    ToggleWorkspace(String),
    NewThread(Option<String>),
    /// Open the create form from the device header.
    NewWorkspace,
    /// Open the edit form pre-filled from the row.
    EditWorkspace(crate::model::WorkspaceView),
    DeleteWorkspace {
        workspace_id: String,
        name: String,
    },
}

/// Map a stored workspace icon name to a bundled glyph. The native editor
/// offers more names than the icon font ships, so unknown values fall back to
/// the generic folder instead of blanking the row.
fn workspace_icon(name: &str) -> crate::icons::Icon {
    match name {
        "monitor" | "desktop" => crate::icons::Icon::Desktop,
        "cloud" => crate::icons::Icon::Cloud,
        "camera" => crate::icons::Icon::Camera,
        "robot" | "agent" => crate::icons::Icon::Agent,
        "gear" => crate::icons::Icon::Settings,
        "flask" | "tool" => crate::icons::Icon::Tool,
        "database" => crate::icons::Icon::Database,
        _ => crate::icons::Icon::Folder,
    }
}

struct RenameThreadDialog {
    session_id: String,
    original_title: String,
    draft: String,
}

/// Bounded overview of a thread's durable chat/process projection. The full
/// storage-backed trajectory is deliberately loaded through the selected
/// thread's normal history path, so this window never creates a second
/// unbounded event-log cache in the UI process.
struct ThreadTrajectoryDialog {
    session_id: String,
    title: String,
}

struct PendingThreadRename {
    session_id: String,
    original_title: String,
    requested_title: String,
}

struct WorkspacePromptDialog {
    kind: WorkspacePromptKind,
    draft: String,
}

#[derive(Clone)]
enum WorkspacePromptKind {
    Directory,
    File,
    /// Rename one entry in place; `path` stays the old path so the reply can
    /// refresh the same directory even if the name changed.
    Rename {
        path: String,
    },
}

/// Deferred intent from the working-directory panel. Returning intents keeps
/// the scroll closure free of `&mut self` calls and makes each path testable.
enum FilesAction {
    Select(String),
    Open(String),
    /// Expand or collapse a directory. Expanding an unloaded directory also
    /// requests its children.
    Toggle(String),
    Refresh,
    /// Drop every expansion and re-read the root. Refreshing a tree in place
    /// would leave cached subdirectories pointing at deleted paths.
    CollapseAll,
    CreateDirectory,
    CreateFile,
    Rename {
        path: String,
        name: String,
    },
    Delete {
        path: String,
        name: String,
        is_directory: bool,
    },
    /// Batch delete for multi-select; always confirmed first.
    DeleteSelected,
    Reference(String),
    OpenRootWorkspace,
    EnterMultiSelect,
    ExitMultiSelect,
    ToggleSelection(String),
    SelectAllVisible,
}

/// Destructive entries always pass through this dialog; the backend delete
/// command itself never asks, so the confirmation must live above it.
/// Destructive entries always pass through this dialog; the backend delete
/// command itself never asks. Multi-select reuses it with several items, so the
/// wording and the confirmation rule cannot diverge between the two paths.
struct DeleteEntryConfirm {
    items: Vec<DeleteEntryItem>,
}

struct DeleteEntryItem {
    path: String,
    name: String,
    is_directory: bool,
}

/// Create/edit form for a workspace row.
///
/// 蛹 opens a full editor (name, folder, icon, colour); the same four values
/// are what the façade's `WorkspaceEdit` takes, so the dialog never needs a
/// second round trip to read the current binding back.
struct WorkspaceDialog {
    /// Empty means "create"; the façade treats it the same way.
    workspace_id: String,
    name: String,
    root_path: String,
    icon: String,
    color: String,
    /// Façade validation message, shown in place instead of a status line.
    error: String,
    /// A save is in flight. The form stays open on failure so the user can fix
    /// the value that was rejected instead of retyping the whole thing.
    busy: bool,
}

struct WorkspaceDeleteConfirm {
    workspace_id: String,
    name: String,
    /// `true` removes the thread records; `false` archives them.
    delete_threads: bool,
}

/// In-flight workspace reorder. Only the dragged row's id is kept; the slot
/// under the pointer is derived from geometry, never stored, so a stale value
/// cannot survive a re-layout.
#[derive(Clone)]
struct WorkspaceDrag {
    workspace_id: String,
}

/// What one workspace row reports back: the intent its buttons raised, plus the
/// row's own response so the caller can watch it for a reorder drag.
struct WorkspaceRowOutcome {
    action: Option<SidebarRowAction>,
    response: egui::Response,
}

/// Icon choices offered by the workspace dialog, in the same order as the
/// native `WORKSPACE_ICONS` subset this shell can draw.
const WORKSPACE_ICON_CHOICES: &[&str] = &[
    "folder", "monitor", "camera", "robot", "gear", "flask", "database", "cloud",
];

/// Colour choices offered by the workspace dialog. Names must stay identical
/// to the native `WORKSPACE_COLORS`; `theme::workspace_color` resolves them.
const WORKSPACE_COLOR_CHOICES: &[&str] = &[
    "red",
    "green",
    "blue",
    "brown",
    "crimson",
    "pink",
    "purple",
    "yellow",
    "orange",
    "teal",
    "bright-blue",
    "black",
    "light-purple",
    "light-green",
    "gray",
];

/// What the composer strip asked for this frame.
///
/// The strip is drawn with `&self` because it sits inside the composer's
/// borrowed `ui`; collection still has to wait until that borrow ends.
#[derive(Default)]
struct ComposerStripAction {
    remove_attachment: Option<String>,
    remove_queued: Option<usize>,
    recall_queued: Option<usize>,
}

enum PendingModelChange {
    Model {
        session_id: String,
        previous: Option<String>,
    },
    Reasoning {
        session_id: String,
        previous: String,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComposerSubmenu {
    Commands,
    Presets,
    Approval,
    Screenshot,
}

fn toggle_composer_submenu(
    current: Option<ComposerSubmenu>,
    requested: ComposerSubmenu,
) -> Option<ComposerSubmenu> {
    if current == Some(requested) {
        None
    } else {
        Some(requested)
    }
}

pub struct EguiApp {
    backend: Box<dyn BackendPort>,
    perf: PerfCounters,
    pub state: AppState,
    styled: bool,
    last_selected: Option<String>,
    follow_tail: bool,
    /// Turn the right-edge ruler asked to reveal. Consumed by the next message
    /// body pass, because the scroll area owns the offsets.
    scroll_to_turn: Option<usize>,
    /// When the current composer notice was raised, in egui seconds. The notice
    /// is transient feedback, not model state, so the expiry lives with the
    /// view instead of `AppState`.
    composer_notice_at: f64,
    /// The notice text the timer above belongs to. Notices are set from a dozen
    /// call sites; watching for a change here keeps each of them from having to
    /// know about timing.
    composer_notice_seen: String,
    composer_menu_open: bool,
    composer_submenu: Option<ComposerSubmenu>,
    approval_mode: &'static str,
    terminal_mode: bool,
    sidebar_open: bool,
    sidebar_split: f32,
    rename_thread_dialog: Option<RenameThreadDialog>,
    rename_thread_needs_focus: bool,
    trajectory_dialog: Option<ThreadTrajectoryDialog>,
    pending_thread_renames: Vec<PendingThreadRename>,
    /// Multi-select state for the thread list. The rules live in
    /// `thread_list` so the pointer and keyboard paths share one tested model.
    thread_selection: crate::thread_list::ThreadSelection,
    workspace_prompt: Option<WorkspacePromptDialog>,
    workspace_prompt_needs_focus: bool,
    delete_entry_confirm: Option<DeleteEntryConfirm>,
    /// Create/edit form for a workspace row. One dialog serves both because the
    /// façade takes the same edit for either; an empty id means create.
    workspace_dialog: Option<WorkspaceDialog>,
    workspace_dialog_needs_focus: bool,
    /// Deleting a workspace also decides what happens to its threads, so the
    /// confirmation carries that choice instead of asking twice.
    workspace_delete_confirm: Option<WorkspaceDeleteConfirm>,
    /// A workspace row being dragged to a new position. The insertion slot is
    /// recomputed from the row spans every frame, so it stays correct while the
    /// list scrolls under a stationary pointer.
    workspace_drag: Option<WorkspaceDrag>,
    /// The directory list owns Arrow/Enter/F2/Delete/F5 until a text editor
    /// takes focus back. egui's widget focus alone is not stable enough: the
    /// list is a scroll area, so arrow keys may re-target focus.
    files_keyboard_active: bool,
    pending_model_change: Option<PendingModelChange>,
}

/// Shared menu body for a directory row. Both the right-click context menu
/// and the hover "more" button use it, so the two pointer paths cannot drift.
fn file_menu_items(
    ui: &mut Ui,
    path: &str,
    name: &str,
    is_directory: bool,
    tokens: Tokens,
) -> Option<FilesAction> {
    ui.set_min_width(WORKSPACE_MENU_MIN_WIDTH);
    let mut action = None;
    if ui
        .button(if is_directory {
            "展开 / 收起"
        } else {
            "打开 / 预览"
        })
        .clicked()
    {
        action = Some(if is_directory {
            FilesAction::Toggle(path.to_owned())
        } else {
            FilesAction::Open(path.to_owned())
        });
        ui.close();
    }
    if ui.button("引用到输入框").clicked() {
        action = Some(FilesAction::Reference(path.to_owned()));
        ui.close();
    }
    ui.separator();
    if ui.button("重命名").on_hover_text("F2").clicked() {
        action = Some(FilesAction::Rename {
            path: path.to_owned(),
            name: name.to_owned(),
        });
        ui.close();
    }
    let delete = ui
        .add(egui::Button::new(
            RichText::new("删除").color(tokens.danger),
        ))
        .on_hover_text("Delete");
    if delete.clicked() {
        action = Some(FilesAction::Delete {
            path: path.to_owned(),
            name: name.to_owned(),
            is_directory,
        });
        ui.close();
    }
    ui.separator();
    if ui.button("新建文件").clicked() {
        action = Some(FilesAction::CreateFile);
        ui.close();
    }
    if ui.button("新建目录").clicked() {
        action = Some(FilesAction::CreateDirectory);
        ui.close();
    }
    ui.separator();
    // Upload, download and directory archiving still need a native dialog or a
    // façade call that does not exist. They stay visible but disabled so the
    // menu documents the gap instead of pretending to work.
    ui.add_enabled(false, egui::Button::new("上传文件"))
        .on_disabled_hover_text("需要原生文件选择对话框，暂未接入");
    ui.add_enabled(false, egui::Button::new("下载"))
        .on_disabled_hover_text("需要原生另存对话框，暂未接入");
    ui.add_enabled(false, egui::Button::new("归档目录"))
        .on_disabled_hover_text("后端尚未提供目录归档能力");
    ui.separator();
    if ui.button("刷新").on_hover_text("F5").clicked() {
        action = Some(FilesAction::Refresh);
        ui.close();
    }
    action
}

fn format_tokens(value: u32) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f32 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}k", value as f32 / 1_000.0)
    } else {
        value.to_string()
    }
}

impl EguiApp {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native(ctx: Context) -> Self {
        let perf = PerfCounters::new();
        let wake_ctx = ctx.clone();
        let waker: crate::backend::UiWaker = Arc::new(move || wake_ctx.request_repaint());
        let mut app = Self {
            backend: Box::new(crate::backend::NativeBackend::start(waker, perf.clone())),
            perf,
            state: AppState::default(),
            styled: false,
            last_selected: None,
            follow_tail: true,
            scroll_to_turn: None,
            composer_notice_at: 0.0,
            composer_notice_seen: String::new(),
            composer_menu_open: false,
            composer_submenu: None,
            approval_mode: "执行前确认",
            terminal_mode: false,
            sidebar_open: false,
            sidebar_split: DEFAULT_SIDEBAR_SPLIT,
            rename_thread_dialog: None,
            rename_thread_needs_focus: false,
            trajectory_dialog: None,
            pending_thread_renames: Vec::new(),
            thread_selection: crate::thread_list::ThreadSelection::default(),
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            delete_entry_confirm: None,
            workspace_dialog: None,
            workspace_dialog_needs_focus: false,
            workspace_delete_confirm: None,
            workspace_drag: None,
            files_keyboard_active: false,
            pending_model_change: None,
        };
        let _ = app.backend.submit(BackendCommand::RefreshSessions);
        // Startup fallback only. Ready/Sessions/Workspaces wake the UI; this
        // timer just keeps a broken runtime from freezing the splash forever.
        ctx.request_repaint_after(IDLE_CLOCK_REPAINT);
        app
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn demo() -> Self {
        let state = crate::fixtures::demo_state();
        let last_selected = state.selected_session.clone();
        Self {
            backend: Box::new(crate::backend::DemoBackend::default()),
            perf: PerfCounters::new(),
            follow_tail: state.should_follow_tail,
            scroll_to_turn: None,
            composer_notice_at: 0.0,
            composer_notice_seen: String::new(),
            state,
            styled: false,
            // The fixed session is already materialized in `demo_state`; do not
            // submit an OpenSession command on the first frame.
            last_selected,
            composer_menu_open: false,
            composer_submenu: None,
            approval_mode: "执行前确认",
            terminal_mode: false,
            sidebar_open: false,
            sidebar_split: DEFAULT_SIDEBAR_SPLIT,
            rename_thread_dialog: None,
            rename_thread_needs_focus: false,
            trajectory_dialog: None,
            pending_thread_renames: Vec::new(),
            thread_selection: crate::thread_list::ThreadSelection::default(),
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            delete_entry_confirm: None,
            workspace_dialog: None,
            workspace_dialog_needs_focus: false,
            workspace_delete_confirm: None,
            workspace_drag: None,
            files_keyboard_active: false,
            pending_model_change: None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn web(ctx: Context) -> Self {
        let perf = PerfCounters::new();
        let wake_ctx = ctx.clone();
        let waker: crate::backend::UiWaker = Arc::new(move || wake_ctx.request_repaint());
        let app = Self {
            backend: Box::new(crate::backend::RemoteBackend::start(waker, perf.clone())),
            perf,
            state: AppState::default(),
            styled: false,
            last_selected: None,
            follow_tail: true,
            scroll_to_turn: None,
            composer_notice_at: 0.0,
            composer_notice_seen: String::new(),
            composer_menu_open: false,
            composer_submenu: None,
            approval_mode: "执行前确认",
            terminal_mode: false,
            sidebar_open: false,
            sidebar_split: DEFAULT_SIDEBAR_SPLIT,
            rename_thread_dialog: None,
            rename_thread_needs_focus: false,
            trajectory_dialog: None,
            pending_thread_renames: Vec::new(),
            thread_selection: crate::thread_list::ThreadSelection::default(),
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            delete_entry_confirm: None,
            workspace_dialog: None,
            workspace_dialog_needs_focus: false,
            workspace_delete_confirm: None,
            workspace_drag: None,
            files_keyboard_active: false,
            pending_model_change: None,
        };
        // The remote adapter also wakes through `request_repaint`; this is the
        // startup fallback while the first connect result is in flight.
        ctx.request_repaint_after(IDLE_CLOCK_REPAINT);
        app
    }

    fn pump(&mut self, ctx: &Context) {
        let events = crate::backend::coalesce_delta_events(self.backend.drain(EVENT_DRAIN_BUDGET));
        // A saturated budget means the queue still holds work. Without a
        // follow-up frame those events would wait for the next input; wakes
        // are not re-armed after the drain.
        let drain_saturated = events.len() == EVENT_DRAIN_BUDGET;
        self.perf.mark_events_applied(events.len());
        for event in events {
            match event {
                crate::backend::BackendEvent::ThreadRenamed {
                    session_id,
                    title,
                    result,
                } => self.finish_thread_rename(session_id, title, result),
                crate::backend::BackendEvent::SessionModelSaved {
                    session_id,
                    model_key,
                    result,
                } => self.finish_session_model_save(session_id, model_key, result),
                crate::backend::BackendEvent::SessionReasoningEffortSaved {
                    session_id,
                    value,
                    result,
                } => self.finish_session_reasoning_save(session_id, value, result),
                crate::backend::BackendEvent::Sessions(mut sessions) => {
                    // A catalog refresh can arrive between local edits. Keep
                    // their optimistic projections until the façade result
                    // arrives, avoiding a stale-label/model flicker.
                    for pending in &self.pending_thread_renames {
                        if let Some(session) = sessions
                            .iter_mut()
                            .find(|session| session.id == pending.session_id)
                        {
                            session.title.clone_from(&pending.requested_title);
                        }
                    }
                    if let Some(PendingModelChange::Model {
                        session_id,
                        previous: _,
                    }) = &self.pending_model_change
                    {
                        if let Some(pinned) = self
                            .state
                            .sessions
                            .iter()
                            .find(|session| session.id == *session_id)
                            .and_then(|session| session.model_key.clone())
                        {
                            if let Some(session) = sessions
                                .iter_mut()
                                .find(|session| session.id == *session_id)
                            {
                                session.model_key = Some(pinned);
                            }
                        }
                    }
                    apply_event(
                        &mut self.state,
                        crate::backend::BackendEvent::Sessions(sessions),
                    );
                }
                crate::backend::BackendEvent::SessionCreated { result } => match result {
                    Ok(session) => {
                        let id = session.id.clone();
                        self.state.sessions.insert(0, session);
                        self.select_session(id);
                        // The new row changes its workspace's thread count, so
                        // the rail's middle level has to be re-read.
                        self.submit(BackendCommand::RefreshWorkspaces);
                    }
                    Err(error) => self.state.status = format!("无法新建线程：{error}"),
                },
                crate::backend::BackendEvent::Ready => {
                    apply_event(&mut self.state, crate::backend::BackendEvent::Ready);
                    // The rail cannot draw a thread until it knows its
                    // workspace, and readiness is the first moment the
                    // catalogue is readable.
                    self.submit(BackendCommand::RefreshWorkspaces);
                }
                crate::backend::BackendEvent::WorkspaceAction { label, result } => match result {
                    Ok(()) => {
                        self.state.status = label;
                        // A save that the façade accepted closes its form; a
                        // failure keeps it open with the message inline. Other
                        // actions sharing this channel (a file write, a
                        // reorder) leave an idle form alone.
                        if self
                            .workspace_dialog
                            .as_ref()
                            .is_some_and(|dialog| dialog.busy)
                        {
                            self.workspace_dialog = None;
                        }
                        self.refresh_workspace_files();
                        // A workspace edit and a file write share this
                        // channel. Re-reading both catalogues is cheap on this
                        // user-action path and keeps the rail truthful: a
                        // rename changes the thread's workspace label, and a
                        // delete can remove or archive its threads.
                        self.submit(BackendCommand::RefreshWorkspaces);
                        self.submit(BackendCommand::RefreshSessions);
                    }
                    Err(error) => {
                        self.state.status = error.clone();
                        // A failed reorder leaves the optimistic order the drag
                        // applied, so the catalogue has to be re-read to put the
                        // rows back where the backend says they belong.
                        if let Some(dialog) = self.workspace_dialog.as_mut() {
                            dialog.busy = false;
                            dialog.error = error;
                        } else {
                            self.submit(BackendCommand::RefreshWorkspaces);
                        }
                    }
                },
                crate::backend::BackendEvent::SessionsArchived { archived, failed } => {
                    let counts_changed = !archived.is_empty();
                    apply_event(
                        &mut self.state,
                        crate::backend::BackendEvent::SessionsArchived { archived, failed },
                    );
                    if counts_changed {
                        // The workspace rows print a thread count, so an
                        // archive has to refresh the middle level as well.
                        self.submit(BackendCommand::RefreshWorkspaces);
                    }
                }
                event => apply_event(&mut self.state, event),
            }
        }
        // A queued prompt starts the moment the running turn lets go. The
        // reducer owns `busy`, so this is the single place that decides when
        // the queue may advance.
        if !self.state.busy
            && !self.state.prompt_queue.is_empty()
            && self.state.selected_session.is_some()
            && self.state.connection == ConnectionState::Ready
        {
            self.drain_prompt_queue();
        }
        // Content frames are driven by the backend waker. Keep only a 1 Hz
        // clock while starting or running so elapsed time and transient
        // notices do not freeze during silent intervals.
        if let Some(delay) =
            scheduled_frame_delay(self.state.connection, self.state.busy, drain_saturated)
        {
            ctx.request_repaint_after(delay);
        }
    }

    fn submit(&mut self, command: BackendCommand) {
        if let Err(error) = self.backend.submit(command) {
            self.state.status = error;
        }
    }

    /// Counters for the event-driven path. Read by probes and tests; they
    /// never contain message text, paths or user identifiers.
    pub fn perf_snapshot(&self) -> crate::perf::PerfSnapshot {
        self.perf.snapshot()
    }

    fn finish_session_model_save(
        &mut self,
        session_id: String,
        model_key: String,
        result: Result<(), String>,
    ) {
        let previous = match self.pending_model_change.take() {
            Some(PendingModelChange::Model {
                session_id: pending_id,
                previous,
            }) if pending_id == session_id => Some(previous),
            Some(other) => {
                self.pending_model_change = Some(other);
                None
            }
            None => None,
        };
        let is_current = self.state.selected_session.as_deref() == Some(session_id.as_str());
        if is_current {
            self.state.model_picker_saving = false;
        }
        match result {
            Ok(()) => {
                if let Some(session) = self
                    .state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.model_key = (!model_key.trim().is_empty()).then_some(model_key);
                }
                if is_current {
                    self.state.model_picker_error.clear();
                }
            }
            Err(error) => {
                if let Some(previous) = previous {
                    if let Some(session) = self
                        .state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.model_key = previous;
                    }
                }
                if is_current {
                    self.state.model_picker_error = error;
                }
            }
        }
    }

    fn finish_session_reasoning_save(
        &mut self,
        session_id: String,
        _value: String,
        result: Result<String, String>,
    ) {
        let is_current = self.state.selected_session.as_deref() == Some(session_id.as_str());
        if is_current {
            self.state.model_picker_saving = false;
        }
        let previous = match self.pending_model_change.take() {
            Some(PendingModelChange::Reasoning {
                session_id: pending_id,
                previous,
            }) if pending_id == session_id => Some(previous),
            Some(other) => {
                self.pending_model_change = Some(other);
                None
            }
            None => None,
        };
        match result {
            Ok(saved) => {
                if let Some(session) = self
                    .state
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.reasoning_effort = saved;
                }
                if is_current {
                    self.state.model_picker_error.clear();
                }
            }
            Err(error) => {
                if let Some(previous) = previous {
                    if let Some(session) = self
                        .state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.reasoning_effort = previous;
                    }
                }
                if is_current {
                    self.state.model_picker_error = error;
                }
            }
        }
    }

    fn select_session(&mut self, id: String) {
        if self.state.selected_session.as_deref() == Some(id.as_str()) {
            return;
        }
        self.state.select_session(id.clone());
        // A save can finish after switching threads. The completion still
        // updates its owner session, but the new picker must not inherit its
        // spinner or failure text.
        self.state.model_picker_saving = false;
        self.last_selected = Some(id.clone());
        self.follow_tail = true;
        self.submit(BackendCommand::OpenSession(id));
        self.refresh_workspace_files();
    }

    fn draw_sidebar(&mut self, ctx: &Context) {
        let tokens = theme::tokens();
        let screen = ctx.screen_rect();
        let compact = uses_compact_sidebar(screen.width());
        if compact {
            self.draw_compact_sidebar(ctx, screen, tokens);
        } else {
            SidePanel::left("messenger-sidebar")
                .default_width(SIDEBAR_WIDTH)
                .width_range(SIDEBAR_MIN_WIDTH..=SIDEBAR_MAX_WIDTH)
                .resizable(true)
                .show_separator_line(false)
                .frame(Frame::NONE.fill(tokens.sidebar))
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    self.draw_sidebar_contents(ui, tokens);
                });
        }
        self.draw_thread_rename_dialog(ctx, tokens);
        self.draw_thread_trajectory_dialog(ctx, tokens);
        self.draw_workspace_prompt(ctx, tokens);
        self.draw_delete_entry_confirm(ctx, tokens);
        self.draw_workspace_dialog(ctx, tokens);
        self.draw_workspace_delete_confirm(ctx, tokens);
    }

    fn draw_compact_sidebar(&mut self, ctx: &Context, screen: egui::Rect, tokens: Tokens) {
        if !self.sidebar_open {
            let trigger = egui::Rect::from_min_size(
                screen.left_top() + Vec2::new(8.0, 8.0),
                Vec2::splat(30.0),
            );
            let response = egui::Area::new(Id::new("messenger-sidebar-trigger"))
                .order(Order::Foreground)
                .fixed_pos(trigger.min)
                .show(ctx, |ui| {
                    let (rect, response) = ui.allocate_exact_size(trigger.size(), Sense::click());
                    let fill = if response.hovered() {
                        tokens.hover
                    } else {
                        tokens.surface
                    };
                    ui.painter().rect_filled(rect, 8.0, fill);
                    ui.painter().rect_stroke(
                        rect,
                        8.0,
                        Stroke::new(1.0, tokens.border),
                        egui::StrokeKind::Inside,
                    );
                    let line = Stroke::new(1.5, tokens.text_secondary);
                    for y in [rect.top() + 9.0, rect.center().y, rect.bottom() - 9.0] {
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left() + 8.0, y),
                                egui::pos2(rect.right() - 8.0, y),
                            ],
                            line,
                        );
                    }
                    response
                })
                .inner;
            if response.clicked() {
                self.sidebar_open = true;
            }
            return;
        }

        let drawer_width = drawer_sidebar_width(screen.width());
        let scrim = egui::Area::new(Id::new("messenger-sidebar-scrim"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(rect, 0.0, theme::tokens().scrim);
                response
            })
            .inner;
        if scrim.clicked() || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.sidebar_open = false;
            return;
        }

        egui::Area::new(Id::new("messenger-sidebar-drawer"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                Frame::NONE.fill(tokens.sidebar).show(ui, |ui| {
                    let (drawer, _) = ui.allocate_exact_size(
                        Vec2::new(drawer_width, screen.height()),
                        Sense::hover(),
                    );
                    ui.painter().line_segment(
                        [drawer.right_top(), drawer.right_bottom()],
                        Stroke::new(1.0, tokens.border),
                    );
                    ui.scope_builder(UiBuilder::new().max_rect(drawer), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        self.draw_sidebar_contents(ui, tokens);
                    });
                });
            });
    }

    fn draw_sidebar_contents(&mut self, ui: &mut Ui, tokens: Tokens) {
        let full = ui.available_rect_before_wrap();
        let track_h = (full.height() - SIDEBAR_FOOTER_HEIGHT).max(80.0);
        let workspace_h = ((track_h - SIDEBAR_SPLITTER_HEIGHT) * self.sidebar_split).max(48.0);
        let workspace = egui::Rect::from_min_size(full.min, Vec2::new(full.width(), workspace_h));
        let splitter = egui::Rect::from_min_size(
            workspace.left_bottom(),
            Vec2::new(full.width(), SIDEBAR_SPLITTER_HEIGHT),
        );
        let files = egui::Rect::from_min_max(
            splitter.left_bottom(),
            egui::pos2(full.right(), full.bottom() - SIDEBAR_FOOTER_HEIGHT),
        );
        let footer = egui::Rect::from_min_max(files.left_bottom(), full.max);
        // These tracks have fixed bounds: long trees scroll inside their
        // region and cannot displace the directory panel or account footer.
        ui.scope_builder(
            UiBuilder::new()
                .id_salt("workspace-region")
                .max_rect(workspace),
            |ui| {
                ui.set_clip_rect(workspace);
                self.draw_workspace_region(ui, tokens);
            },
        );
        for y in [splitter.top(), splitter.bottom()] {
            ui.painter().line_segment(
                [egui::pos2(full.left(), y), egui::pos2(full.right(), y)],
                Stroke::new(1.0, tokens.border),
            );
        }
        ui.painter().line_segment(
            [
                splitter.center() - Vec2::new(14.0, 0.0),
                splitter.center() + Vec2::new(14.0, 0.0),
            ],
            Stroke::new(2.0, tokens.border_strong),
        );
        let splitter_response = ui.interact(
            splitter,
            Id::new("sidebar-workspace-files-splitter"),
            Sense::click_and_drag(),
        );
        if splitter_response.double_clicked() {
            self.sidebar_split = DEFAULT_SIDEBAR_SPLIT;
        } else if splitter_response.dragged() {
            if let Some(pointer) = splitter_response.interact_pointer_pos() {
                self.sidebar_split = split_from_pointer(pointer.y, full.top(), track_h);
            }
        }
        if splitter_response.hovered() || splitter_response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
        }
        ui.scope_builder(
            UiBuilder::new().id_salt("files-region").max_rect(files),
            |ui| {
                ui.set_clip_rect(files);
                self.draw_files_region(ui, tokens);
            },
        );
        ui.scope_builder(
            UiBuilder::new().id_salt("account-region").max_rect(footer),
            |ui| {
                ui.set_clip_rect(footer);
                self.draw_account_footer(ui, tokens);
            },
        );
        ui.painter().line_segment(
            [full.right_top(), full.right_bottom()],
            Stroke::new(1.0, tokens.border),
        );
        ui.allocate_rect(full, Sense::hover());
    }

    /// Device header: which device owns the tree below it (蛹's top rail row).
    ///
    /// The egui shell only has a local device today, so the row renders the
    /// signed-out shape: monitor glyph, `本地`, host `蛹`, and no chevron. A
    /// cloud target would add the menu; offering an empty device would let a
    /// click land on a tree the backend cannot serve.
    fn draw_device_header(&mut self, ui: &mut Ui, tokens: Tokens) -> Option<SidebarRowAction> {
        let (full, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), crate::sidebar_tree::DEVICE_ROW_HEIGHT),
            Sense::hover(),
        );
        // The header owns the row minus the "new workspace" affordance, which
        // sits on the right edge exactly as the Slint device bar does.
        let plus = egui::Rect::from_center_size(
            egui::pos2(full.right() - 21.0, full.center().y),
            Vec2::splat(22.0),
        );
        let rect = egui::Rect::from_min_max(
            egui::pos2(full.left() + 8.0, full.top()),
            egui::pos2(plus.left() - 2.0, full.bottom()),
        );
        let device = self.state.sidebar.device();
        let switchable = self.state.sidebar.can_switch_device();
        let pointer_in_row = ui.rect_contains_pointer(rect);
        let response = ui.interact(
            rect,
            Id::new("sidebar-device"),
            if switchable {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if switchable {
            response
                .clone()
                .on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        if pointer_in_row && switchable {
            ui.painter().rect_filled(rect, 7.0, tokens.hover);
        }
        let icon = if device.is_cloud() {
            crate::icons::Icon::Cloud
        } else {
            crate::icons::Icon::Desktop
        };
        crate::icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                rect.left_center() + Vec2::new(13.0, 0.0),
                Vec2::splat(15.0),
            ),
            icon,
            if device.is_cloud() {
                tokens.primary
            } else {
                tokens.text_muted
            },
        );
        ui.painter().text(
            rect.left_center() + Vec2::new(28.0, 0.0),
            egui::Align2::LEFT_CENTER,
            device.label(),
            egui::FontId::proportional(15.0),
            tokens.text,
        );
        let host = ui.painter().layout_no_wrap(
            device.host().to_owned(),
            egui::FontId::proportional(11.0),
            tokens.text_muted,
        );
        let host_pos = rect.left_center() + Vec2::new(28.0 + 34.0, 0.0);
        ui.painter().galley(
            host_pos - Vec2::new(0.0, host.size().y / 2.0),
            host,
            tokens.text_muted,
        );
        if switchable {
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 8.0, rect.center().y),
                    Vec2::splat(12.0),
                ),
                crate::icons::Icon::ChevronDown,
                tokens.text_muted,
            );
        }
        if response.clicked() && switchable {
            // Two devices, one tree each: clicking the header flips to the
            // other device, which is what the 蛹 menu's only entries do.
            let next = if device.is_cloud() {
                crate::sidebar_tree::Device::Local
            } else {
                crate::sidebar_tree::Device::Cloud
            };
            self.state.sidebar.set_device(next);
        }
        let tooltip = format!("{} · {}", device.label(), device.host());
        response.on_hover_text(tooltip);
        let plus_response = ui
            .interact(plus, Id::new("sidebar-new-workspace"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("新建工作区");
        if plus_response.hovered() {
            ui.painter().rect_filled(plus, 6.0, tokens.hover);
        }
        crate::icons::paint(
            ui.painter(),
            plus.shrink(5.0),
            crate::icons::Icon::Plus,
            if plus_response.hovered() {
                tokens.primary
            } else {
                tokens.text_muted
            },
        );
        plus_response
            .clicked()
            .then_some(SidebarRowAction::NewWorkspace)
    }

    /// One workspace row: 34px, washed glyph, name, live thread count and a
    /// hover-only action band, as in 蛹.
    ///
    /// The row's own response is handed back because it doubles as the drag
    /// handle: a press that turns into a move reorders the rail, a press that
    /// stays put toggles the group.
    fn draw_workspace_row(
        ui: &mut Ui,
        workspace: &crate::model::WorkspaceView,
        expanded: bool,
        thread_count: usize,
        dragging: bool,
        tokens: Tokens,
    ) -> WorkspaceRowOutcome {
        let (full, _) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width(),
                crate::sidebar_tree::WORKSPACE_ROW_HEIGHT,
            ),
            Sense::hover(),
        );
        let rect = full.shrink2(Vec2::new(6.0, 0.0));
        // `click_and_drag` is what makes a press that turns into a move report
        // `dragged()` instead of silently swallowing the gesture: with plain
        // `Sense::click` a reorder drag would never start.
        let response = ui
            .interact(
                rect,
                Id::new(("workspace-row", &workspace.id)),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // The row under the pointer reads as lifted while it is being dragged,
        // so the drop slot stays legible.
        if dragging {
            ui.painter().rect_filled(rect, 7.0, tokens.selected);
        }
        let pointer_in_row = ui.rect_contains_pointer(rect);
        // Expanded groups read as "open": a wash of the workspace colour behind
        // the glyph. Collapsed groups fall back to the neutral hover fill, the
        // same fold cue the Slint row uses (no separate chevron).
        let tint = theme::workspace_color(&workspace.color);
        ui.painter().rect_filled(
            rect,
            if pointer_in_row { 7.0 } else { 0.0 },
            if pointer_in_row && !expanded {
                tokens.hover
            } else {
                Color32::TRANSPARENT
            },
        );
        // Selection indicator: the open group is the selected workspace, marked
        // by the same 2px accent bar 蛹 paints at the row's left edge.
        if expanded {
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 5.0),
                Vec2::new(2.0, 24.0),
            );
            ui.painter().rect_filled(bar, 1.0, tokens.primary);
        }
        let glyph = egui::Rect::from_center_size(
            rect.left_center() + Vec2::new(23.0, 0.0),
            Vec2::splat(22.0),
        );
        ui.painter().rect_filled(
            glyph,
            6.0,
            if expanded {
                theme::workspace_wash(tint)
            } else {
                tokens.hover
            },
        );
        crate::icons::paint(
            ui.painter(),
            glyph.shrink(4.0),
            workspace_icon(&workspace.icon),
            if expanded { tint } else { tokens.text_muted },
        );
        let plus = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 18.0, rect.center().y),
            Vec2::splat(22.0),
        );
        // The rename and delete affordances ride left of the new-thread button
        // and only exist while the pointer is on the row, so the name and the
        // thread count never reflow. The spacing matches 蛹's 74px action band
        // (+ at 42, pen at 26, trash at 0).
        let pen = egui::Rect::from_center_size(
            egui::pos2(plus.left() - 16.0, rect.center().y),
            Vec2::splat(22.0),
        );
        let trash = egui::Rect::from_center_size(
            egui::pos2(plus.left() - 42.0, rect.center().y),
            Vec2::splat(22.0),
        );
        // The band is the hovered area's backdrop: it rides over the row rather
        // than joining its layout, exactly like the Slint rectangle.
        if pointer_in_row {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(trash.left(), rect.top()),
                    egui::pos2(rect.right(), rect.bottom()),
                ),
                7.0,
                if expanded {
                    tokens.selected
                } else {
                    tokens.primary_tint
                },
            );
        }
        let count_text = format!("{thread_count}");
        let count = ui.painter().layout_no_wrap(
            count_text,
            egui::FontId::proportional(11.0),
            tokens.text_muted,
        );
        // The count yields to the hover affordance instead of shifting it.
        if !pointer_in_row {
            ui.painter().galley(
                egui::pos2(
                    plus.left() - 8.0 - count.size().x,
                    rect.center().y - count.size().y / 2.0,
                ),
                count,
                tokens.text_muted,
            );
        }
        let title = ui.painter().layout_no_wrap(
            workspace.name.clone(),
            egui::FontId::proportional(15.0),
            tokens.text,
        );
        let title_rect = egui::Rect::from_min_max(
            egui::pos2(glyph.right() + 8.0, rect.top()),
            egui::pos2(trash.left() - 4.0, rect.bottom()),
        );
        ui.painter().with_clip_rect(title_rect).galley(
            egui::pos2(title_rect.left(), rect.center().y - title.size().y / 2.0),
            title,
            tokens.text,
        );
        let mut action = None;
        let plus_response = ui
            .interact(
                plus,
                Id::new(("workspace-new-thread", &workspace.id)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("在工作区中新建线程");
        let pen_response = ui
            .interact(
                pen,
                Id::new(("workspace-edit", &workspace.id)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("编辑工作区");
        let trash_response = ui
            .interact(
                trash,
                Id::new(("workspace-delete", &workspace.id)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("删除工作区");
        if pointer_in_row {
            if trash_response.hovered() {
                ui.painter().rect_filled(trash, 6.0, tokens.selected);
            }
            crate::icons::paint(
                ui.painter(),
                trash.shrink(5.0),
                crate::icons::Icon::Trash,
                if trash_response.hovered() {
                    tokens.danger
                } else {
                    tokens.text_muted
                },
            );
            if pen_response.hovered() {
                ui.painter().rect_filled(pen, 6.0, tokens.selected);
            }
            crate::icons::paint(
                ui.painter(),
                pen.shrink(5.0),
                crate::icons::Icon::Pen,
                if pen_response.hovered() {
                    tokens.text
                } else {
                    tokens.text_muted
                },
            );
            if plus_response.hovered() {
                ui.painter().rect_filled(plus, 6.0, tokens.surface);
            }
            crate::icons::paint(
                ui.painter(),
                plus.shrink(5.0),
                crate::icons::Icon::Plus,
                if plus_response.hovered() {
                    tokens.primary
                } else {
                    tokens.text_muted
                },
            );
        }
        if plus_response.clicked() {
            action = Some(SidebarRowAction::NewThread(Some(workspace.id.clone())));
        } else if pen_response.clicked() {
            action = Some(SidebarRowAction::EditWorkspace(workspace.clone()));
        } else if trash_response.clicked() {
            action = Some(SidebarRowAction::DeleteWorkspace {
                workspace_id: workspace.id.clone(),
                name: workspace.name.clone(),
            });
        } else if response.clicked() {
            action = Some(SidebarRowAction::ToggleWorkspace(workspace.id.clone()));
        }
        WorkspaceRowOutcome { action, response }
    }

    /// Draw the rail's middle and lowest levels.
    fn draw_workspace_region(&mut self, ui: &mut Ui, tokens: Tokens) {
        ui.add_space(8.0);
        let mut action = self.draw_device_header(ui, tokens);
        let selection_bar_height = if self.thread_selection.is_active() {
            38.0
        } else {
            0.0
        };
        let available = (ui.available_height() - selection_bar_height).max(0.0);
        // One note covers both empty shapes: an unavailable device and a
        // device with no workspace yet. The header stays interactive either
        // way, so its "new workspace" intent still has to be handled.
        let note = if self.state.sidebar.device().is_cloud() {
            Some("云端工作区尚未接入")
        } else if self.state.workspaces_loaded && self.state.workspaces.is_empty() {
            Some("暂无工作区")
        } else {
            None
        };
        if let Some(note) = note {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(note).size(12.0).color(tokens.text_muted));
            });
            if let Some(action) = action {
                self.handle_sidebar_row_action(action);
            }
            return;
        }
        let rows = self
            .state
            .sidebar
            .rows(&self.state.workspaces, &self.state.sessions);
        let selected = self.state.selected_session.clone();
        let selecting = self.thread_selection.is_active();
        // The rail bounds a reorder: a pointer that wanders into the message
        // list must not resolve a slot from coordinates the rail never drew.
        let rail = ui.max_rect();
        // Screen-space spans of the workspace rows, in list order. Threads are
        // skipped on purpose: a reorder slot is measured against the rows that
        // actually move.
        let mut spans: Vec<(f32, f32)> = Vec::new();
        let mut drag_started: Option<String> = None;
        let mut drag_released = false;
        ScrollArea::vertical()
            .id_salt("sidebar-tree")
            .auto_shrink([false, false])
            .max_height(available)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &rows {
                    match row {
                        crate::sidebar_tree::Row::Workspace {
                            workspace,
                            expanded,
                        } => {
                            let count = crate::sidebar_tree::thread_count(
                                &self.state.sessions,
                                &workspace.id,
                            );
                            let top = ui.cursor().top();
                            let dragging = self
                                .workspace_drag
                                .as_ref()
                                .is_some_and(|drag| drag.workspace_id == workspace.id);
                            let outcome = Self::draw_workspace_row(
                                ui, workspace, *expanded, count, dragging, tokens,
                            );
                            spans.push((top, top + crate::sidebar_tree::WORKSPACE_ROW_HEIGHT));
                            if let Some(next) = outcome.action {
                                action = Some(next);
                            }
                            if outcome.response.drag_started() {
                                drag_started = Some(workspace.id.clone());
                            }
                            if dragging && outcome.response.drag_stopped() {
                                drag_released = true;
                            }
                        }
                        crate::sidebar_tree::Row::Thread { index, session } => {
                            // The connector line links the thread to the group
                            // above it, as the Slint row does at x = 20px.
                            let top = ui.cursor().top();
                            let line_x = ui.cursor().left() + 20.0;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(line_x, top),
                                    egui::pos2(
                                        line_x,
                                        top + crate::sidebar_tree::THREAD_ROW_HEIGHT,
                                    ),
                                ],
                                Stroke::new(1.0, tokens.border),
                            );
                            let checked = selecting && self.thread_selection.contains(&session.id);
                            if let Some(next) = Self::draw_thread_row(
                                ui,
                                &self.state.sessions[*index],
                                selected.as_deref(),
                                selecting,
                                checked,
                                tokens,
                            ) {
                                action = Some(SidebarRowAction::Thread(next));
                            }
                        }
                    }
                }
                // The insertion marker has to be painted inside the scroll area
                // or the clip rect would swallow it the moment the list scrolls.
                if self.workspace_drag.is_some() {
                    if let Some(slot) = Self::pointer_slot(ui, &spans, rail) {
                        // A slot past the last row draws under it; otherwise the
                        // line sits on top of the row the drop would precede.
                        let y = spans.get(slot).map_or_else(
                            || spans.last().map_or(0.0, |(_, bottom)| *bottom),
                            |(top, _)| *top,
                        );
                        let left = ui.max_rect().left() + 10.0;
                        let right = ui.max_rect().right() - 10.0;
                        ui.painter().line_segment(
                            [egui::pos2(left, y), egui::pos2(right, y)],
                            Stroke::new(2.0, tokens.primary),
                        );
                    }
                }
            });
        if let Some(workspace_id) = drag_started {
            self.workspace_drag = Some(WorkspaceDrag { workspace_id });
        }
        if let Some(drag) = self.workspace_drag.clone() {
            // `drag_released` covers a release over the row; a primary-button-up
            // check covers a release anywhere else, so a drag that ends off the
            // rail still resolves instead of hanging.
            let released = drag_released || !ui.input(|input| input.pointer.primary_down());
            if released {
                self.commit_workspace_reorder(&drag.workspace_id, &spans, rail, ui);
                self.workspace_drag = None;
            }
        }
        if self.thread_selection.is_active() {
            self.draw_thread_selection_bar(ui, tokens);
        }
        if let Some(action) = action {
            self.handle_sidebar_row_action(action);
        }
    }

    /// Insertion slot under the pointer, in workspace-row order. `None` means
    /// the pointer is outside the rail, so a drag started there must not move
    /// anything.
    fn pointer_slot(ui: &Ui, spans: &[(f32, f32)], rail: egui::Rect) -> Option<usize> {
        let pointer = ui.input(|input| input.pointer.hover_pos())?;
        if !rail.contains(pointer) {
            return None;
        }
        crate::sidebar_tree::drop_index(spans, pointer.y)
    }

    /// Apply a finished drag: move the row locally so the rail answers at once,
    /// then ask the backend to persist the same order.
    fn commit_workspace_reorder(
        &mut self,
        workspace_id: &str,
        spans: &[(f32, f32)],
        rail: egui::Rect,
        ui: &Ui,
    ) {
        let ids: Vec<String> = self
            .state
            .workspaces
            .iter()
            .map(|workspace| workspace.id.clone())
            .collect();
        let Some(slot) = Self::pointer_slot(ui, spans, rail) else {
            return;
        };
        let Some(order) = crate::sidebar_tree::moved_order(&ids, workspace_id, slot) else {
            return;
        };
        if order == ids {
            return;
        }
        self.state.workspaces.sort_by_key(|workspace| {
            order
                .iter()
                .position(|id| id == &workspace.id)
                .unwrap_or(usize::MAX)
        });
        self.submit(BackendCommand::ReorderWorkspaces { ids: order });
    }

    fn handle_sidebar_row_action(&mut self, action: SidebarRowAction) {
        match action {
            SidebarRowAction::Thread(action) => self.handle_thread_row_action(action),
            SidebarRowAction::ToggleWorkspace(workspace_id) => {
                self.state.sidebar.toggle(&workspace_id);
            }
            SidebarRowAction::NewThread(workspace_id) => {
                self.submit(BackendCommand::CreateSession { workspace_id });
            }
            SidebarRowAction::NewWorkspace => {
                self.workspace_dialog = Some(WorkspaceDialog {
                    workspace_id: String::new(),
                    name: String::new(),
                    root_path: String::new(),
                    icon: "folder".to_owned(),
                    color: "blue".to_owned(),
                    error: String::new(),
                    busy: false,
                });
                self.workspace_dialog_needs_focus = true;
            }
            SidebarRowAction::EditWorkspace(workspace) => {
                self.workspace_dialog = Some(WorkspaceDialog {
                    workspace_id: workspace.id,
                    name: workspace.name,
                    root_path: workspace.root_path,
                    icon: workspace.icon,
                    color: workspace.color,
                    error: String::new(),
                    busy: false,
                });
                self.workspace_dialog_needs_focus = true;
            }
            SidebarRowAction::DeleteWorkspace { workspace_id, name } => {
                self.workspace_delete_confirm = Some(WorkspaceDeleteConfirm {
                    workspace_id,
                    name,
                    // Archiving is the reversible default, as in 蛹.
                    delete_threads: false,
                });
            }
        }
    }

    fn draw_files_region(&mut self, ui: &mut Ui, tokens: Tokens) {
        let full = ui.max_rect();
        // Register the directory background before the rows. egui gives later
        // widgets precedence, so a row keeps its click/selection input instead
        // of the panel's context-menu hitbox consuming it.
        let directory_response = ui
            .interact(full, Id::new("working-directory-menu"), Sense::click())
            .on_hover_text("右键打开工作目录菜单")
            .on_hover_cursor(egui::CursorIcon::ContextMenu);
        let mut action: Option<FilesAction> = None;

        let head = egui::Rect::from_min_size(full.min, Vec2::new(full.width(), 24.0));
        let refresh_rect = egui::Rect::from_center_size(
            egui::pos2(full.right() - 14.0, head.center().y),
            Vec2::splat(18.0),
        );
        let select_rect = egui::Rect::from_center_size(
            egui::pos2(refresh_rect.left() - 6.0, head.center().y),
            Vec2::splat(18.0),
        );
        let title_rect = egui::Rect::from_min_max(
            egui::pos2(head.left() + 12.0, head.top()),
            egui::pos2(select_rect.left() - 4.0, head.bottom()),
        );
        let multi = self.state.workspace_selection.is_active();
        let title = if multi {
            format!("已选 {} 项", self.state.workspace_selection.len())
        } else {
            "工作目录".to_owned()
        };
        // A fully clipped title produces an empty mesh, which some glow
        // backends reject; skip degenerate rects instead of painting nothing.
        if title_rect.width() > 1.0 {
            ui.painter().with_clip_rect(title_rect).text(
                title_rect.left_center(),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(12.0),
                if multi {
                    tokens.text_secondary
                } else {
                    tokens.text_muted
                },
            );
        }
        let refresh = ui
            .interact(refresh_rect, Id::new("workspace-refresh"), Sense::click())
            .on_hover_text("刷新（F5）")
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if refresh.hovered() {
            ui.painter().rect_filled(refresh_rect, 4.0, tokens.hover);
        }
        crate::icons::paint(
            ui.painter(),
            refresh_rect.shrink(4.0),
            crate::icons::Icon::Refresh,
            tokens.text_muted,
        );
        if refresh.clicked() {
            action = Some(FilesAction::Refresh);
        }
        let select = ui
            .interact(
                select_rect,
                Id::new("workspace-multi-select"),
                Sense::click(),
            )
            .on_hover_text(if multi {
                "退出多选（Esc）"
            } else {
                "多选"
            })
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if select.hovered() || multi {
            ui.painter().rect_filled(
                select_rect,
                4.0,
                if multi { tokens.selected } else { tokens.hover },
            );
        }
        crate::icons::paint(
            ui.painter(),
            select_rect.shrink(4.0),
            crate::icons::Icon::CircleCheck,
            if multi {
                tokens.primary
            } else {
                tokens.text_muted
            },
        );
        if select.clicked() {
            action = Some(if multi {
                FilesAction::ExitMultiSelect
            } else {
                FilesAction::EnterMultiSelect
            });
        }

        // The selection bar is a fixed strip: rows must not scroll behind it.
        let bar_height = if multi { 30.0 } else { 0.0 };
        let body = egui::Rect::from_min_max(
            head.left_bottom(),
            egui::pos2(
                full.right(),
                (full.bottom() - bar_height).max(head.bottom()),
            ),
        );
        ui.painter().rect_filled(body, 0.0, tokens.surface);
        ui.painter().rect_stroke(
            body,
            0.0,
            Stroke::new(1.0, tokens.border),
            egui::StrokeKind::Inside,
        );
        if self.state.workspace_file_loading {
            ui.painter().text(
                body.center(),
                egui::Align2::CENTER_CENTER,
                "正在读取工作目录…",
                egui::FontId::proportional(12.0),
                tokens.text_muted,
            );
        } else if !self.state.workspace_file_error.is_empty() {
            ui.painter().text(
                body.center() - Vec2::new(0.0, 10.0),
                egui::Align2::CENTER_CENTER,
                &self.state.workspace_file_error,
                egui::FontId::proportional(12.0),
                tokens.danger,
            );
            ui.painter().text(
                body.center() + Vec2::new(0.0, 14.0),
                egui::Align2::CENTER_CENTER,
                "右键刷新或打开本地工作区",
                egui::FontId::proportional(11.0),
                tokens.text_muted,
            );
        } else if self.state.workspace_files.is_empty() {
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    body.center() - Vec2::new(0.0, 22.0),
                    Vec2::splat(20.0),
                ),
                crate::icons::Icon::FolderOpen,
                tokens.text_muted,
            );
            ui.painter().text(
                body.center() + Vec2::new(0.0, 6.0),
                egui::Align2::CENTER_CENTER,
                "当前目录没有文件",
                egui::FontId::proportional(12.0),
                tokens.text_muted,
            );
            ui.painter().text(
                body.center() + Vec2::new(0.0, 30.0),
                egui::Align2::CENTER_CENTER,
                "右键新建文件或目录",
                egui::FontId::proportional(11.0),
                tokens.text_muted,
            );
        } else if let Some(next) =
            self.draw_tree_rows(ui, body.shrink2(Vec2::new(6.0, 3.0)), tokens)
        {
            action = Some(next);
        }

        if bar_height > 0.0 {
            if let Some(next) = self.draw_selection_bar(ui, full, bar_height, tokens) {
                action = Some(next);
            }
        }

        directory_response.context_menu(|ui| {
            ui.set_min_width(WORKSPACE_MENU_MIN_WIDTH);
            if ui.button("新建目录").clicked() {
                action = Some(FilesAction::CreateDirectory);
                ui.close();
            }
            if ui.button("新建文件").clicked() {
                action = Some(FilesAction::CreateFile);
                ui.close();
            }
            ui.separator();
            if ui.button("打开本地工作区").clicked() {
                action = Some(FilesAction::OpenRootWorkspace);
                ui.close();
            }
            ui.separator();
            // Upload and download need a native file dialog the façade does not
            // have; they stay visible but disabled instead of pretending.
            ui.add_enabled(false, egui::Button::new("上传文件"))
                .on_disabled_hover_text("需要原生文件选择对话框，暂未接入");
            ui.add_enabled(false, egui::Button::new("下载"))
                .on_disabled_hover_text("需要原生另存对话框，暂未接入");
            ui.separator();
            if ui.button("全部收起").clicked() {
                action = Some(FilesAction::CollapseAll);
                ui.close();
            }
            if ui.button("刷新").on_hover_text("F5").clicked() {
                action = Some(FilesAction::Refresh);
                ui.close();
            }
        });

        if let Some(action) = action {
            self.apply_files_action(action);
        }
    }

    /// Multi-select bar: count on the left, destructive action on the right.
    fn draw_selection_bar(
        &mut self,
        ui: &mut Ui,
        full: egui::Rect,
        bar_height: f32,
        tokens: Tokens,
    ) -> Option<FilesAction> {
        let bar = egui::Rect::from_min_max(
            egui::pos2(full.left(), full.bottom() - bar_height),
            full.max,
        );
        ui.painter().rect_filled(bar, 0.0, tokens.sidebar);
        ui.painter().line_segment(
            [bar.left_top(), bar.right_top()],
            Stroke::new(1.0, tokens.border),
        );
        let mut action = None;
        let count = self.state.workspace_selection.len();
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(bar.shrink2(Vec2::new(6.0, 4.0)))
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.label(
                    RichText::new(if count == 0 {
                        "选择要操作的文件".to_owned()
                    } else {
                        format!("已选 {count} 项")
                    })
                    .size(11.0)
                    .color(tokens.text_muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let delete = ui.add_enabled(
                        count > 0,
                        egui::Button::new(RichText::new("删除").color(tokens.danger)),
                    );
                    if delete.clicked() {
                        action = Some(FilesAction::DeleteSelected);
                    }
                    if ui.button("全选").clicked() {
                        action = Some(FilesAction::SelectAllVisible);
                    }
                });
            },
        );
        action
    }

    /// Draw the tree. Rows come from `workspace_tree::flatten`, so the pointer
    /// and keyboard paths walk the exact same sequence the painter used.
    fn draw_tree_rows(
        &mut self,
        ui: &mut Ui,
        area: egui::Rect,
        tokens: Tokens,
    ) -> Option<FilesAction> {
        let mut action: Option<FilesAction> = None;
        let mut claim_keys = false;
        {
            let rows = self
                .state
                .workspace_tree
                .flatten(&self.state.workspace_files);
            let selected = self.state.selected_workspace_file.clone();
            let multi = self.state.workspace_selection.is_active();
            let list = ui.interact(area, Id::new("workspace-file-list"), Sense::click());
            // Keyboard ownership is tracked on the app, not through egui focus:
            // a scroll area re-targets focus on arrow keys, and a text editor
            // must always keep the keys it owns. Clicking the list claims them,
            // clicking anywhere else releases them.
            let clicked_elsewhere = ui.ctx().input(|input| input.pointer.any_click())
                && !ui.rect_contains_pointer(area);
            if clicked_elsewhere {
                self.files_keyboard_active = false;
            } else if list.clicked() {
                claim_keys = true;
            }
            if self.files_keyboard_active {
                ui.painter().rect_stroke(
                    area,
                    5.0,
                    Stroke::new(1.0, tokens.primary_soft),
                    egui::StrokeKind::Inside,
                );
            }
            ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                ScrollArea::vertical()
                    .id_salt("workspace-file-list")
                    .auto_shrink([false, false])
                    .show_rows(
                        ui,
                        crate::workspace_tree::ROW_HEIGHT,
                        rows.len(),
                        |ui, range| {
                            for index in range {
                                let Some(row) = rows.get(index) else {
                                    continue;
                                };
                                let (row_rect, response) = ui.allocate_exact_size(
                                    Vec2::new(
                                        ui.available_width(),
                                        crate::workspace_tree::ROW_HEIGHT,
                                    ),
                                    Sense::click(),
                                );
                                let pointer_in_row = ui.rect_contains_pointer(row_rect);
                                let is_selected = selected.as_deref() == Some(row.path);
                                let is_checked = self.state.workspace_selection.contains(row.path);
                                if is_selected || is_checked || pointer_in_row {
                                    ui.painter().rect_filled(
                                        row_rect,
                                        5.0,
                                        if is_checked || is_selected {
                                            tokens.selected
                                        } else {
                                            tokens.hover
                                        },
                                    );
                                }
                                let center_y = row_rect.center().y;
                                let mut x =
                                    row_rect.left() + crate::workspace_tree::indent(row.depth);
                                if multi {
                                    let check = egui::Rect::from_center_size(
                                        egui::pos2(x + 6.0, center_y),
                                        Vec2::splat(13.0),
                                    );
                                    ui.painter().rect_filled(check, 3.0, tokens.surface);
                                    ui.painter().rect_stroke(
                                        check,
                                        3.0,
                                        Stroke::new(
                                            1.0,
                                            if is_checked {
                                                tokens.primary
                                            } else {
                                                tokens.border_strong
                                            },
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    if is_checked {
                                        crate::icons::paint(
                                            ui.painter(),
                                            check.shrink(2.5),
                                            crate::icons::Icon::Check,
                                            tokens.primary,
                                        );
                                    }
                                    x += 18.0;
                                }
                                // Directories get a twisty; files keep the same slot
                                // empty so names line up column-wise.
                                if row.is_directory {
                                    let twisty = egui::Rect::from_center_size(
                                        egui::pos2(x + 6.0, center_y),
                                        Vec2::splat(12.0),
                                    );
                                    let twisty_response = ui.interact(
                                        twisty,
                                        Id::new(("workspace-twisty", row.path)),
                                        Sense::click(),
                                    );
                                    if twisty_response.hovered() {
                                        ui.painter().rect_filled(twisty, 3.0, tokens.hover);
                                    }
                                    crate::icons::paint(
                                        ui.painter(),
                                        twisty.shrink(1.0),
                                        if row.expanded {
                                            crate::icons::Icon::ChevronDown
                                        } else {
                                            crate::icons::Icon::ChevronRight
                                        },
                                        tokens.text_muted,
                                    );
                                    if twisty_response.clicked() {
                                        action = Some(FilesAction::Toggle(row.path.to_owned()));
                                    }
                                }
                                x += crate::workspace_tree::INDENT_STEP.min(14.0);
                                crate::icons::paint(
                                    ui.painter(),
                                    egui::Rect::from_center_size(
                                        egui::pos2(x + 7.0, center_y),
                                        Vec2::splat(crate::workspace_tree::ICON_SIZE),
                                    ),
                                    if row.is_directory {
                                        if row.expanded {
                                            crate::icons::Icon::FolderOpen
                                        } else {
                                            crate::icons::Icon::Folder
                                        }
                                    } else {
                                        crate::icons::Icon::File
                                    },
                                    if row.is_directory {
                                        tokens.warning
                                    } else {
                                        tokens.text_muted
                                    },
                                );
                                x += 18.0;
                                // The hover action button owns a fixed 20px slot, so
                                // revealing it never shifts the name or the size.
                                let more = egui::Rect::from_center_size(
                                    egui::pos2(row_rect.right() - 13.0, center_y),
                                    Vec2::splat(20.0),
                                );
                                let trailing_width =
                                    if row.loading || row.failed { 52.0 } else { 0.0 };
                                let name_rect = egui::Rect::from_min_max(
                                    egui::pos2(x, row_rect.top()),
                                    egui::pos2(
                                        more.left() - 4.0 - trailing_width,
                                        row_rect.bottom(),
                                    ),
                                );
                                if name_rect.width() > 1.0 {
                                    let galley = ui.painter().layout(
                                        row.name.to_owned(),
                                        egui::FontId::proportional(12.0),
                                        tokens.text_secondary,
                                        name_rect.width(),
                                    );
                                    ui.painter().with_clip_rect(name_rect).galley(
                                        name_rect.left_center()
                                            - Vec2::new(0.0, galley.size().y / 2.0),
                                        galley,
                                        tokens.text_secondary,
                                    );
                                }
                                if row.loading {
                                    ui.painter().text(
                                        egui::pos2(more.left() - 4.0, center_y),
                                        egui::Align2::RIGHT_CENTER,
                                        "读取中",
                                        egui::FontId::proportional(10.0),
                                        tokens.text_muted,
                                    );
                                } else if row.failed {
                                    ui.painter().text(
                                        egui::pos2(more.left() - 4.0, center_y),
                                        egui::Align2::RIGHT_CENTER,
                                        "读取失败",
                                        egui::FontId::proportional(10.0),
                                        tokens.danger,
                                    );
                                } else if !row.is_directory && row.size > 0 && !pointer_in_row {
                                    ui.painter().text(
                                        egui::Rect::from_center_size(
                                            egui::pos2(more.left() - 4.0, center_y),
                                            Vec2::splat(1.0),
                                        )
                                        .left_center(),
                                        egui::Align2::RIGHT_CENTER,
                                        format!("{} B", row.size),
                                        egui::FontId::proportional(10.0),
                                        tokens.text_muted,
                                    );
                                }
                                // The "more" button keeps its hit region every frame:
                                // a popup closes as soon as its owner stops being
                                // shown, so hiding it on pointer-out would dismiss
                                // the menu the pointer is moving into.
                                let more_response = ui
                                    .interact(
                                        more,
                                        Id::new(("workspace-more", row.path)),
                                        Sense::click(),
                                    )
                                    .on_hover_text("更多操作")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if pointer_in_row || more_response.hovered() {
                                    ui.painter().rect_filled(more, 4.0, tokens.card_border);
                                    crate::icons::paint(
                                        ui.painter(),
                                        more.shrink(5.0),
                                        crate::icons::Icon::More,
                                        tokens.text_muted,
                                    );
                                }
                                egui::Popup::menu(&more_response)
                                    .id(Id::new(("workspace-more-menu", row.path)))
                                    .show(|ui| {
                                        if let Some(chosen) = file_menu_items(
                                            ui,
                                            row.path,
                                            row.name,
                                            row.is_directory,
                                            tokens,
                                        ) {
                                            action = Some(chosen);
                                        }
                                    });
                                if response.double_clicked() {
                                    action = Some(if row.is_directory {
                                        FilesAction::Toggle(row.path.to_owned())
                                    } else {
                                        FilesAction::Open(row.path.to_owned())
                                    });
                                } else if response.clicked() {
                                    if multi {
                                        action =
                                            Some(FilesAction::ToggleSelection(row.path.to_owned()));
                                    } else {
                                        action = Some(FilesAction::Select(row.path.to_owned()));
                                    }
                                }
                                response.context_menu(|ui| {
                                    if let Some(chosen) = file_menu_items(
                                        ui,
                                        row.path,
                                        row.name,
                                        row.is_directory,
                                        tokens,
                                    ) {
                                        action = Some(chosen);
                                    }
                                });
                            }
                        },
                    );
            });

            // Keyboard follows the same intent enum as the menus, so pointer and
            // keyboard paths cannot drift apart.
            if self.files_keyboard_active && !ui.ctx().wants_keyboard_input() {
                let keys = ui.input(|input| {
                    (
                        input.key_pressed(egui::Key::ArrowUp),
                        input.key_pressed(egui::Key::ArrowDown),
                        input.key_pressed(egui::Key::Home),
                        input.key_pressed(egui::Key::End),
                        input.key_pressed(egui::Key::Enter),
                        input.key_pressed(egui::Key::F2),
                        input.key_pressed(egui::Key::Delete),
                        input.key_pressed(egui::Key::F5),
                        input.key_pressed(egui::Key::ArrowLeft),
                        input.key_pressed(egui::Key::ArrowRight),
                        input.key_pressed(egui::Key::Escape),
                        input.key_pressed(egui::Key::Space),
                    )
                });
                let (
                    up,
                    down,
                    home,
                    end,
                    enter,
                    rename,
                    delete,
                    refresh,
                    left,
                    right,
                    escape,
                    space,
                ) = keys;
                let current = rows
                    .iter()
                    .position(|row| selected.as_deref() == Some(row.path));
                let step = |delta: isize| -> Option<&crate::workspace_tree::TreeRow<'_>> {
                    if rows.is_empty() {
                        return None;
                    }
                    let len = rows.len() as isize;
                    let base = current.map_or(-1, |index| index as isize);
                    rows.get((base + delta).clamp(0, len - 1) as usize)
                };
                if escape && multi {
                    action = Some(FilesAction::ExitMultiSelect);
                } else if up {
                    action = step(-1).map(|row| FilesAction::Select(row.path.to_owned()));
                } else if down {
                    action = step(1).map(|row| FilesAction::Select(row.path.to_owned()));
                } else if home {
                    action = rows
                        .first()
                        .map(|row| FilesAction::Select(row.path.to_owned()));
                } else if end {
                    action = rows
                        .last()
                        .map(|row| FilesAction::Select(row.path.to_owned()));
                } else if refresh {
                    action = Some(FilesAction::Refresh);
                } else if let Some(row) = current.and_then(|index| rows.get(index)) {
                    if left {
                        action = Some(if row.is_directory && row.expanded {
                            FilesAction::Toggle(row.path.to_owned())
                        } else {
                            // Collapsed row: move up to the parent instead of
                            // doing nothing, which is what the web tree does.
                            FilesAction::Select(row.path.to_owned())
                        });
                    } else if right && row.is_directory {
                        action = Some(FilesAction::Toggle(row.path.to_owned()));
                    } else if space && multi {
                        action = Some(FilesAction::ToggleSelection(row.path.to_owned()));
                    } else if enter {
                        action = Some(if row.is_directory {
                            FilesAction::Toggle(row.path.to_owned())
                        } else {
                            FilesAction::Open(row.path.to_owned())
                        });
                    } else if rename {
                        action = Some(FilesAction::Rename {
                            path: row.path.to_owned(),
                            name: row.name.to_owned(),
                        });
                    } else if delete {
                        action = Some(FilesAction::Delete {
                            path: row.path.to_owned(),
                            name: row.name.to_owned(),
                            is_directory: row.is_directory,
                        });
                    }
                }
            }
        }
        if claim_keys {
            self.files_keyboard_active = true;
        }
        action
    }

    fn apply_files_action(&mut self, action: FilesAction) {
        match action {
            FilesAction::Select(path) => self.state.select_workspace_file(path),
            FilesAction::Open(path) => self.open_workspace_file(path),
            FilesAction::Toggle(path) => {
                // `toggle` answers "do the children still need loading", so a
                // second expand of a cached folder costs no round trip.
                let needs_load = self.state.workspace_tree.toggle(&path);
                if needs_load {
                    self.state.workspace_tree.set_loading(&path);
                    if let Some(session_id) = self.state.selected_session.clone() {
                        self.submit(BackendCommand::ListWorkspaceChildren { session_id, path });
                    }
                }
            }
            FilesAction::Refresh => {
                self.state.workspace_tree.collapse_all();
                self.state.workspace_selection.exit();
                self.refresh_workspace_files();
            }
            FilesAction::CollapseAll => {
                self.state.workspace_tree.collapse_all();
                self.state.status = "已收起全部目录".to_owned();
            }
            FilesAction::CreateDirectory => {
                self.workspace_prompt = Some(WorkspacePromptDialog {
                    kind: WorkspacePromptKind::Directory,
                    draft: String::new(),
                });
                self.workspace_prompt_needs_focus = true;
            }
            FilesAction::CreateFile => {
                self.workspace_prompt = Some(WorkspacePromptDialog {
                    kind: WorkspacePromptKind::File,
                    draft: String::new(),
                });
                self.workspace_prompt_needs_focus = true;
            }
            FilesAction::Rename { path, name } => {
                self.workspace_prompt = Some(WorkspacePromptDialog {
                    kind: WorkspacePromptKind::Rename { path },
                    draft: name,
                });
                self.workspace_prompt_needs_focus = true;
            }
            FilesAction::Delete {
                path,
                name,
                is_directory,
            } => {
                self.delete_entry_confirm = Some(DeleteEntryConfirm {
                    items: vec![DeleteEntryItem {
                        path,
                        name,
                        is_directory,
                    }],
                });
            }
            FilesAction::DeleteSelected => {
                let items: Vec<DeleteEntryItem> = self
                    .state
                    .workspace_selection
                    .ids()
                    .into_iter()
                    .filter_map(|path| {
                        let row = self
                            .state
                            .workspace_tree
                            .flatten(&self.state.workspace_files)
                            .into_iter()
                            .find(|row| row.path == path)?;
                        Some(DeleteEntryItem {
                            path: row.path.to_owned(),
                            name: row.name.to_owned(),
                            is_directory: row.is_directory,
                        })
                    })
                    .collect();
                if items.is_empty() {
                    self.state.status = "请选择要删除的项".to_owned();
                } else {
                    self.delete_entry_confirm = Some(DeleteEntryConfirm { items });
                }
            }
            FilesAction::Reference(path) => {
                let draft = &mut self.state.composer;
                let separator = if draft.is_empty() || draft.ends_with(char::is_whitespace) {
                    ""
                } else {
                    "\n"
                };
                draft.push_str(separator);
                draft.push_str(&format!("@\"{path}\" "));
                self.state.status = format!("已引用 {path}");
            }
            FilesAction::OpenRootWorkspace => {
                if let Some(session_id) = self.state.selected_session.clone() {
                    self.submit(BackendCommand::OpenLocalWorkspace(session_id));
                } else {
                    self.state.status = "请先选择一个线程".to_owned();
                }
            }
            FilesAction::EnterMultiSelect => self.state.workspace_selection.enter(),
            FilesAction::ExitMultiSelect => self.state.workspace_selection.exit(),
            FilesAction::ToggleSelection(path) => {
                self.state.workspace_selection.toggle(&path);
            }
            FilesAction::SelectAllVisible => {
                let paths: Vec<String> = self
                    .state
                    .workspace_tree
                    .flatten(&self.state.workspace_files)
                    .into_iter()
                    .map(|row| row.path.to_owned())
                    .collect();
                if self.state.workspace_selection.len() == paths.len() {
                    // Second press on "全选" clears, matching the thread list.
                    self.state.workspace_selection.clear();
                } else {
                    self.state
                        .workspace_selection
                        .select_all(paths.iter().map(String::as_str));
                }
            }
        }
    }

    fn draw_account_footer(&mut self, ui: &mut Ui, tokens: Tokens) {
        let rect = ui.max_rect();
        let response = ui
            .interact(rect, Id::new("account-settings"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("设置");
        if response.hovered() {
            ui.painter().rect_filled(rect, 0.0, tokens.hover);
        }
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, tokens.border),
        );
        let center = rect.left_center() + Vec2::new(20.0, 0.0);
        ui.painter().circle_filled(center, 12.0, tokens.avatar);
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            "本",
            egui::FontId::proportional(11.0),
            tokens.on_accent,
        );
        ui.painter().text(
            rect.left_center() + Vec2::new(40.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "本地用户",
            egui::FontId::proportional(13.0),
            tokens.text_secondary,
        );
        if response.hovered() {
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 18.0, rect.center().y),
                    Vec2::splat(13.0),
                ),
                crate::icons::Icon::Settings,
                tokens.text_muted,
            );
        }
        if response.clicked() {
            self.state.settings_open = true;
        }
    }

    fn draw_thread_row(
        ui: &mut Ui,
        session: &SessionView,
        selected_session: Option<&str>,
        selecting: bool,
        checked: bool,
        tokens: Tokens,
    ) -> Option<ThreadRowAction> {
        let selected = selected_session == Some(session.id.as_str());
        let (full, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
        let rect = full.shrink2(Vec2::new(6.0, 0.0));
        // The action strip always owns this rectangle, even before it is
        // painted. Keeping the hit regions stable prevents a fast first click
        // after entering the row from falling through to thread selection.
        let action_band = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 76.0, rect.top()),
            Vec2::new(76.0, rect.height()),
        );
        let main_rect = if selecting {
            rect
        } else {
            egui::Rect::from_min_max(rect.min, egui::pos2(action_band.left(), rect.bottom()))
        };
        let pointer_in_row = ui.rect_contains_pointer(rect);
        let response = ui
            .interact(main_rect, Id::new(("thread", &session.id)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        // Child icon buttons receive their own interactions later in this
        // pass. `Response::hovered` becomes false when they cover the row,
        // which made the action band alternate between visible and hidden as
        // the pointer crossed it. Geometry is stable even while a child owns
        // the pointer, so it is the right source for this paint-only state.
        let fill = if selected {
            tokens.selected
        } else if pointer_in_row {
            tokens.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 7.0, fill);
        if selected {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    rect.min + Vec2::new(2.0, 6.0),
                    egui::pos2(rect.left() + 4.0, rect.bottom() - 6.0),
                ),
                2.0,
                tokens.primary,
            );
        }
        let marker = egui::Rect::from_center_size(
            rect.left_center() + Vec2::new(if selecting { 48.0 } else { 31.0 }, 0.0),
            Vec2::splat(10.0),
        );
        if selecting {
            let check = egui::Rect::from_center_size(
                rect.left_center() + Vec2::new(16.0, 0.0),
                Vec2::splat(15.0),
            );
            ui.painter().rect(
                check,
                4.0,
                if checked {
                    tokens.primary
                } else {
                    Color32::TRANSPARENT
                },
                Stroke::new(
                    1.0,
                    if checked {
                        tokens.primary
                    } else {
                        tokens.border_strong
                    },
                ),
                egui::StrokeKind::Inside,
            );
            if checked {
                crate::icons::paint(
                    ui.painter(),
                    check.shrink(3.0),
                    crate::icons::Icon::Check,
                    tokens.on_accent,
                );
            }
        }
        match session.status.as_str() {
            "done" | "completed" => crate::icons::paint(
                ui.painter(),
                marker,
                crate::icons::Icon::Check,
                tokens.success,
            ),
            "running" => {
                ui.painter()
                    .circle_stroke(marker.center(), 4.0, Stroke::new(1.5, tokens.primary));
            }
            "failed" | "error" => crate::icons::paint(
                ui.painter(),
                marker,
                crate::icons::Icon::Warning,
                tokens.danger,
            ),
            _ => {
                ui.painter()
                    .circle_filled(marker.center(), 2.0, tokens.text_muted);
            }
        }
        let mut action = None;
        let mut context_action = None;
        let draw_thread_context_menu =
            |ui: &mut Ui, context_action: &mut Option<ThreadRowAction>| {
                ui.set_min_width(THREAD_MENU_MIN_WIDTH);
                if ui.button("重命名").on_hover_text("F2").clicked() {
                    *context_action = Some(ThreadRowAction::Rename {
                        id: session.id.clone(),
                        title: session.title.clone(),
                    });
                    ui.close();
                }
                if ui.button("轨迹视图").clicked() {
                    *context_action = Some(ThreadRowAction::Trajectory {
                        id: session.id.clone(),
                        title: session.title.clone(),
                    });
                    ui.close();
                }
                let archive = ui.add_enabled(!session.locked, egui::Button::new("归档"));
                if archive.clicked() {
                    *context_action = Some(ThreadRowAction::Archive(session.id.clone()));
                    ui.close();
                }
                ui.separator();
                if ui.button("多选").clicked() {
                    *context_action = Some(ThreadRowAction::BeginMultiSelect);
                    ui.close();
                }
            };
        response.context_menu(|ui| draw_thread_context_menu(ui, &mut context_action));
        // The icon controls below take precedence because they are registered
        // later, while the thin gaps in their fixed action band still retain
        // the same right-click menu as the rest of the thread row.
        if !selecting {
            let action_band_response = ui.interact(
                action_band,
                Id::new(("thread-action-band", &session.id)),
                Sense::click(),
            );
            action_band_response
                .context_menu(|ui| draw_thread_context_menu(ui, &mut context_action));
        }
        let title_rect = egui::Rect::from_min_max(
            egui::pos2(
                rect.left() + if selecting { 62.0 } else { 45.0 },
                rect.top(),
            ),
            // Always reserve the hover-action band. Painting it only on
            // hover now cannot expose or conceal a different title suffix.
            egui::pos2(rect.right() - 84.0, rect.bottom()),
        );
        let galley = ui.painter().layout(
            session.title.clone(),
            egui::FontId::proportional(13.0),
            if selected {
                tokens.text
            } else {
                tokens.text_secondary
            },
            f32::INFINITY,
        );
        ui.painter().with_clip_rect(title_rect).galley(
            title_rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
            galley,
            tokens.text_secondary,
        );
        if !selecting {
            let band = action_band;
            if pointer_in_row {
                ui.painter().rect_filled(
                    band,
                    0.0,
                    if selected {
                        tokens.selected
                    } else {
                        tokens.primary_tint
                    },
                );
            }
            for (index, icon, tooltip) in [
                (0, crate::icons::Icon::Pen, "重命名"),
                (1, crate::icons::Icon::List, "轨迹视图"),
                (2, crate::icons::Icon::Archive, "归档"),
            ] {
                let button = egui::Rect::from_center_size(
                    egui::pos2(band.left() + 12.0 + index as f32 * 26.0, band.center().y),
                    Vec2::splat(20.0),
                );
                let enabled = !selecting && (index != 2 || !session.locked);
                let action_response = ui
                    .interact(
                        button,
                        Id::new(("thread-hover-action", &session.id, index)),
                        if enabled {
                            Sense::click()
                        } else {
                            Sense::hover()
                        },
                    )
                    .on_hover_cursor(if enabled {
                        egui::CursorIcon::PointingHand
                    } else {
                        egui::CursorIcon::Default
                    })
                    .on_hover_text(tooltip);
                action_response
                    .context_menu(|ui| draw_thread_context_menu(ui, &mut context_action));
                if pointer_in_row && action_response.hovered() && enabled {
                    // The band sits on `selected` or `primary_tint`, so the
                    // button highlight must step away from whichever fill is
                    // underneath instead of reusing the same token.
                    ui.painter().rect_filled(
                        button,
                        4.0,
                        if selected {
                            tokens.hover
                        } else {
                            tokens.primary_fill
                        },
                    );
                }
                if pointer_in_row {
                    crate::icons::paint(
                        ui.painter(),
                        button.shrink(3.5),
                        icon,
                        if enabled {
                            tokens.text_muted
                        } else {
                            tokens.border_strong
                        },
                    );
                }
                if action_response.clicked() {
                    action = Some(match index {
                        0 => ThreadRowAction::Rename {
                            id: session.id.clone(),
                            title: session.title.clone(),
                        },
                        1 => ThreadRowAction::Trajectory {
                            id: session.id.clone(),
                            title: session.title.clone(),
                        },
                        _ => ThreadRowAction::Archive(session.id.clone()),
                    });
                }
            }
        }
        context_action.or(action).or_else(|| {
            response.clicked().then(|| {
                if selecting {
                    ThreadRowAction::ToggleSelection(session.id.clone())
                } else {
                    ThreadRowAction::Select(session.id.clone())
                }
            })
        })
    }

    fn handle_thread_row_action(&mut self, action: ThreadRowAction) {
        match action {
            ThreadRowAction::Select(id) => self.select_session(id),
            ThreadRowAction::Rename { id, title } => {
                self.rename_thread_dialog = Some(RenameThreadDialog {
                    session_id: id,
                    original_title: title.clone(),
                    draft: title,
                });
                self.rename_thread_needs_focus = true;
            }
            ThreadRowAction::Trajectory { id, title } => {
                // The native façade exposes a thread's durable process page
                // through the same session-open path as chat history. Switch
                // before showing the dialog so an action on an inactive row
                // cannot leave the trajectory spinner waiting forever.
                if self.state.selected_session.as_deref() != Some(id.as_str()) {
                    self.select_session(id.clone());
                }
                self.trajectory_dialog = Some(ThreadTrajectoryDialog {
                    session_id: id,
                    title,
                });
            }
            ThreadRowAction::Archive(id) => {
                self.archive_threads(vec![id]);
            }
            ThreadRowAction::BeginMultiSelect => self.thread_selection.enter(),
            ThreadRowAction::ToggleSelection(id) => {
                // Locked and live threads stay out of the batch: the native
                // facade cancels an active thread before archiving it, and a
                // locked thread cannot be archived at all.
                let eligible = self.state.sessions.iter().any(|session| {
                    session.id == id
                        && !session.locked
                        && crate::thread_list::archivable(&session.status)
                });
                if !eligible {
                    return;
                }
                if self.thread_selection.is_active() {
                    self.thread_selection.toggle(&id);
                } else {
                    self.thread_selection.activate(&id);
                }
            }
        }
    }

    fn draw_thread_selection_bar(&mut self, ui: &mut Ui, tokens: Tokens) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
        ui.painter().line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, tokens.border),
        );
        // Eligibility is decided once, from the same rule the rows use.
        let eligible: Vec<String> = self
            .state
            .sessions
            .iter()
            .filter(|session| !session.locked && crate::thread_list::archivable(&session.status))
            .map(|session| session.id.clone())
            .collect();
        let all_selected =
            !eligible.is_empty() && eligible.iter().all(|id| self.thread_selection.contains(id));
        let entries: Vec<(String, String)> = self
            .state
            .sessions
            .iter()
            .filter(|session| !session.locked)
            .map(|session| (session.id.clone(), session.status.clone()))
            .collect();
        let targets = self.thread_selection.archive_targets(
            entries
                .iter()
                .map(|(id, status)| (id.as_str(), status.as_str())),
        );
        let skipped = self.thread_selection.skipped(
            entries
                .iter()
                .map(|(id, status)| (id.as_str(), status.as_str())),
        );
        let mut archive_request = None;
        ui.scope_builder(
            UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 5.0))),
            |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .small_button(if all_selected {
                            "取消全选"
                        } else {
                            "全选"
                        })
                        .clicked()
                    {
                        if all_selected {
                            self.thread_selection.clear();
                        } else {
                            self.thread_selection
                                .select_all(eligible.iter().map(String::as_str));
                        }
                    }
                    if ui.small_button("退出").clicked() {
                        self.thread_selection.exit();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let hint = if skipped.is_empty() {
                            "归档选中线程".to_owned()
                        } else {
                            // Say what is being left out instead of silently
                            // dropping the running rows from the batch.
                            format!("已跳过 {} 个运行中的线程", skipped.len())
                        };
                        if ui
                            .add_enabled(!targets.is_empty(), egui::Button::new("归档"))
                            .on_hover_text(hint)
                            .clicked()
                        {
                            archive_request = Some(targets.clone());
                        }
                        ui.label(
                            RichText::new(format!("已选 {} 项", self.thread_selection.len()))
                                .size(11.0)
                                .color(tokens.text_muted),
                        );
                    });
                });
            },
        );
        if let Some(ids) = archive_request {
            self.archive_threads(ids);
        }
    }

    /// Archive every selected thread in one backend command. The reducer owns
    /// the removal and the result text, so the panel does not have to sequence
    /// N requests or track partial progress itself.
    fn archive_threads(&mut self, ids: Vec<String>) {
        let targets: Vec<String> = ids
            .into_iter()
            .filter(|id| {
                self.state.sessions.iter().any(|session| {
                    session.id == *id
                        && !session.locked
                        && crate::thread_list::archivable(&session.status)
                })
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if targets.is_empty() {
            self.state.status = "没有可归档的线程".to_owned();
            return;
        }
        // Keep an active thread: archiving the open one would otherwise leave
        // the chat pane pointing at a transcript that is about to be cleared.
        if targets
            .iter()
            .any(|id| self.state.selected_session.as_deref() == Some(id.as_str()))
        {
            self.state.selected_session = self
                .state
                .sessions
                .iter()
                .find(|session| !targets.contains(&session.id))
                .map(|session| session.id.clone());
            self.last_selected = None;
            self.state.selected_workspace_file = None;
        }
        self.thread_selection.exit();
        self.state.status = format!("正在归档 {} 个线程", targets.len());
        if let Err(error) = self
            .backend
            .submit(BackendCommand::ArchiveSessions { ids: targets })
        {
            self.state.status = format!("无法归档线程：{error}");
        }
    }

    /// Re-read the workspace root. The tree owns everything below it, so this
    /// is the only listing the panel ever requests directly.
    fn refresh_workspace_files(&mut self) {
        let Some(session_id) = self.state.selected_session.clone() else {
            return;
        };
        // Bumped before the command leaves, so a reply for a thread the user
        // already left is rejected by the reducer instead of replacing the
        // current listing.
        let request_id = self.state.begin_workspace_refresh();
        self.state.selected_workspace_file = None;
        self.submit(BackendCommand::RefreshWorkspaceFiles {
            session_id,
            path: String::new(),
            request_id,
        });
    }

    fn open_workspace_file(&mut self, path: String) {
        let Some(session_id) = self.state.selected_session.clone() else {
            self.state.status = "请先选择一个线程".to_owned();
            return;
        };
        self.submit(BackendCommand::OpenWorkspaceFile { session_id, path });
    }

    fn draw_thread_rename_dialog(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(mut dialog) = self.rename_thread_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut close_requested = false;
        let mut confirmed = false;
        egui::Window::new("重命名")
            .id(Id::new(("thread-rename-dialog", &dialog.session_id)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.label(format!("重命名「{}」为：", dialog.original_title));
                ui.add_space(8.0);
                let input = ui.add(
                    TextEdit::singleline(&mut dialog.draft)
                        .id(Id::new(("thread-rename-input", &dialog.session_id)))
                        .desired_width(f32::INFINITY)
                        .hint_text("输入线程名称"),
                );
                if self.rename_thread_needs_focus {
                    input.request_focus();
                    self.rename_thread_needs_focus = false;
                }
                let valid = valid_thread_title(&dialog.draft);
                if !dialog.draft.trim().is_empty() && !valid {
                    ui.label(
                        RichText::new("名称最多 120 个字符，且不能包含控制字符")
                            .size(11.0)
                            .color(tokens.danger),
                    );
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confirm = ui.add_enabled(valid, egui::Button::new("确认"));
                    if confirm.clicked()
                        || (input.lost_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                            && valid)
                    {
                        confirmed = true;
                    }
                    if ui.button("取消").clicked() {
                        close_requested = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close_requested = true;
                }
            });
        if close_requested {
            open = false;
        }
        if confirmed {
            let title = dialog.draft.trim().to_owned();
            if title != dialog.original_title {
                self.rename_thread_optimistically(dialog.session_id, dialog.original_title, title);
            }
        } else if open {
            self.rename_thread_dialog = Some(dialog);
        }
    }

    fn draw_thread_trajectory_dialog(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(dialog) = self.trajectory_dialog.take() else {
            return;
        };
        let mut open = true;
        let same_thread =
            self.state.selected_session.as_deref() == Some(dialog.session_id.as_str());
        let loading = !same_thread || self.state.connection == ConnectionState::Starting;
        let turns = if same_thread {
            Some(&self.state.turns)
        } else {
            None
        };
        let screen = ctx.input(|input| input.screen_rect());
        let width = (screen.width() - 72.0).clamp(360.0, 860.0);
        let height = (screen.height() - 96.0).clamp(260.0, 640.0);
        egui::Window::new("线程轨迹")
            .id(Id::new(("thread-trajectory-dialog", &dialog.session_id)))
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(width, height))
            .min_size(Vec2::new(360.0, 260.0))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&dialog.title)
                            .size(16.0)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("执行时间线")
                                .size(12.0)
                                .color(tokens.text_muted),
                        );
                    });
                });
                ui.separator();
                if loading {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.spinner();
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("正在读取线程轨迹…")
                                .size(12.0)
                                .color(tokens.text_muted),
                        );
                    });
                    return;
                }
                let Some(turns) = turns else {
                    return;
                };
                if turns.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("该线程尚无可回放的执行记录")
                                .size(12.0)
                                .color(tokens.text_muted),
                        );
                    });
                    return;
                }
                ScrollArea::vertical()
                    .id_salt(("thread-trajectory-scroll", &dialog.session_id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, turn) in turns.iter().enumerate() {
                            let title = if turn.user.text.trim().is_empty() {
                                format!("初始化 · {}", index + 1)
                            } else {
                                format!("第 {} 轮 · {}", index + 1, turn.user.text.trim())
                            };
                            egui::CollapsingHeader::new(
                                RichText::new(title).size(13.0).color(tokens.text_secondary),
                            )
                            .id_salt(("thread-trajectory-turn", &dialog.session_id, &turn.id))
                            .default_open(index + 1 == turns.len())
                            .show(ui, |ui| {
                                if turn.workflow.is_empty() {
                                    ui.label(
                                        RichText::new("本轮没有工具或推理步骤")
                                            .size(12.0)
                                            .color(tokens.text_muted),
                                    );
                                } else {
                                    for item in &turn.workflow {
                                        let phase = match item.kind {
                                            crate::model::WorkflowKind::Reasoning => "思考",
                                            crate::model::WorkflowKind::Tool => "工具",
                                            crate::model::WorkflowKind::Text => "输出",
                                            crate::model::WorkflowKind::Unknown => "事件",
                                        };
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(
                                                RichText::new(phase)
                                                    .size(11.0)
                                                    .color(tokens.primary),
                                            );
                                            ui.label(
                                                RichText::new(&item.title)
                                                    .size(12.0)
                                                    .color(tokens.text_secondary),
                                            );
                                            if !item.preview.trim().is_empty() {
                                                ui.label(
                                                    RichText::new(&item.preview)
                                                        .size(11.0)
                                                        .color(tokens.text_muted),
                                                );
                                            }
                                        });
                                    }
                                }
                                if !turn.assistant.text.trim().is_empty() {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new("回复已生成")
                                            .size(11.0)
                                            .color(tokens.success),
                                    );
                                }
                            });
                        }
                    });
            });
        if open {
            self.trajectory_dialog = Some(dialog);
        }
    }

    fn draw_workspace_prompt(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(mut dialog) = self.workspace_prompt.take() else {
            return;
        };
        let mut open = true;
        let mut close_requested = false;
        let mut accepted = false;
        let renaming = matches!(dialog.kind, WorkspacePromptKind::Rename { .. });
        let (title, hint, field_label, dialog_id) = match &dialog.kind {
            WorkspacePromptKind::Directory => (
                "新建目录",
                "目录名，例如 notes/archive",
                "相对当前工作目录输入路径：",
                "workspace-create-dialog",
            ),
            WorkspacePromptKind::File => (
                "新建文件",
                "文件名，例如 notes/example.txt",
                "相对当前工作目录输入路径：",
                "workspace-create-dialog",
            ),
            WorkspacePromptKind::Rename { .. } => (
                "重命名",
                "新名称",
                "只改名称，不改变所在目录：",
                "workspace-rename-dialog",
            ),
        };
        egui::Window::new(title)
            .id(Id::new(dialog_id))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.label(field_label);
                ui.add_space(6.0);
                let input = ui.add(
                    TextEdit::singleline(&mut dialog.draft)
                        .id(Id::new((dialog_id, "input")))
                        .desired_width(f32::INFINITY)
                        .hint_text(hint),
                );
                if self.workspace_prompt_needs_focus {
                    input.request_focus();
                    self.workspace_prompt_needs_focus = false;
                }
                let valid = if renaming {
                    valid_workspace_entry_name(&dialog.draft)
                } else {
                    valid_workspace_entry_path(&dialog.draft)
                };
                if !dialog.draft.trim().is_empty() && !valid {
                    ui.label(
                        RichText::new(if renaming {
                            "名称不能为空，不能包含路径分隔符、控制字符或 .."
                        } else {
                            "请输入相对路径，不能包含 .. 或控制字符"
                        })
                        .size(11.0)
                        .color(tokens.danger),
                    );
                }
                ui.add_space(10.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let confirm = ui.add_enabled(valid, egui::Button::new("确认"));
                    if confirm.clicked()
                        || (input.lost_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter))
                            && valid)
                    {
                        accepted = true;
                    }
                    if ui.button("取消").clicked() {
                        close_requested = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close_requested = true;
                }
            });
        if close_requested {
            open = false;
        }
        if accepted {
            let draft = dialog.draft.trim().to_owned();
            if let Some(session_id) = self.state.selected_session.clone() {
                match &dialog.kind {
                    WorkspacePromptKind::Directory => {
                        self.submit(BackendCommand::CreateWorkspaceDirectory {
                            session_id,
                            path: draft,
                        })
                    }
                    WorkspacePromptKind::File => self.submit(BackendCommand::CreateWorkspaceFile {
                        session_id,
                        path: draft,
                    }),
                    WorkspacePromptKind::Rename { path } => {
                        self.submit(BackendCommand::RenameWorkspaceEntry {
                            session_id,
                            path: path.clone(),
                            new_name: draft,
                        })
                    }
                }
            } else {
                self.state.status = "请先选择一个线程".to_owned();
            }
        } else if open {
            self.workspace_prompt = Some(dialog);
        }
    }

    /// Directory deletion is the only irreversible action in this panel, so it
    /// always asks first and names the exact entry.
    fn draw_delete_entry_confirm(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(confirm) = self.delete_entry_confirm.take() else {
            return;
        };
        let mut open = true;
        let mut close_requested = false;
        let mut accepted = false;
        // The wording has to name what disappears: one entry for a row menu, a
        // count plus the first few names for a multi-select batch.
        let headline = match confirm.items.as_slice() {
            [] => "没有可删除的项".to_owned(),
            [item] => {
                if item.is_directory {
                    format!("确定删除目录「{}」及其全部内容？", item.name)
                } else {
                    format!("确定删除「{}」？", item.name)
                }
            }
            items => {
                let preview: Vec<&str> = items
                    .iter()
                    .take(3)
                    .map(|item| item.name.as_str())
                    .collect();
                let more = items.len().saturating_sub(preview.len());
                if more > 0 {
                    format!(
                        "确定删除 {} 项（{} 等 {} 项）？",
                        items.len(),
                        preview.join("、"),
                        more
                    )
                } else {
                    format!("确定删除 {} 项（{}）？", items.len(), preview.join("、"))
                }
            }
        };
        egui::Window::new("删除确认")
            .id(Id::new("workspace-delete-dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(RichText::new(headline).size(13.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("删除后无法从蛹恢复。")
                        .size(11.0)
                        .color(tokens.text_muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let delete = ui.add(
                        egui::Button::new(RichText::new("删除").color(tokens.on_accent))
                            .fill(tokens.danger),
                    );
                    if delete.clicked() {
                        accepted = true;
                    }
                    if ui.button("取消").clicked() {
                        close_requested = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close_requested = true;
                }
            });
        if close_requested {
            open = false;
        }
        if accepted {
            if let Some(session_id) = self.state.selected_session.clone() {
                // One submit per entry: the façade deletes a single path, and a
                // partial failure must leave the rest of the batch intact.
                let count = confirm.items.len();
                for item in &confirm.items {
                    if self
                        .state
                        .selected_workspace_file
                        .as_deref()
                        .is_some_and(|selected| selected == item.path)
                    {
                        self.state.selected_workspace_file = None;
                    }
                    self.state.workspace_selection.toggle(&item.path);
                    self.submit(BackendCommand::DeleteWorkspaceEntry {
                        session_id: session_id.clone(),
                        path: item.path.clone(),
                    });
                }
                self.state.workspace_selection.exit();
                self.state.status = if count == 1 {
                    "正在删除".to_owned()
                } else {
                    format!("正在删除 {count} 项")
                };
            } else {
                self.state.status = "请先选择一个线程".to_owned();
            }
        } else if open {
            self.delete_entry_confirm = Some(confirm);
        }
    }

    /// Create/edit form for a workspace. The façade re-validates the folder,
    /// the name and the palette on save, so this only refuses obviously
    /// incomplete input and reports the façade's own message in place.
    fn draw_workspace_dialog(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(mut dialog) = self.workspace_dialog.take() else {
            return;
        };
        let creating = dialog.workspace_id.is_empty();
        let mut open = true;
        let mut close_requested = false;
        let mut accepted = false;
        egui::Window::new(if creating { "新建工作区" } else { "编辑工作区" })
            .id(Id::new("workspace-edit-workspace-dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(420.0);
                ui.label("名称");
                let name_input = ui.add(
                    TextEdit::singleline(&mut dialog.name)
                        .id(Id::new("workspace-dialog-name"))
                        .desired_width(f32::INFINITY)
                        .hint_text("工作区名称"),
                );
                if self.workspace_dialog_needs_focus {
                    name_input.request_focus();
                    self.workspace_dialog_needs_focus = false;
                }
                ui.add_space(8.0);
                ui.label("目录");
                ui.add(
                    TextEdit::singleline(&mut dialog.root_path)
                        .id(Id::new("workspace-dialog-root"))
                        .desired_width(f32::INFINITY)
                        .hint_text("/path/to/folder"),
                );
                // No native folder picker is linked into this shell, so the
                // dialog also accepts a folder dropped from the file manager:
                // egui reports it on the same channel the composer uses, and
                // peeking keeps that path working.
                let dropped = ui.input(|input| {
                    input.raw.dropped_files.iter().find_map(|file| {
                        let path = file.path.clone()?;
                        // `DroppedFile` carries no kind flag; `metadata` is the
                        // only way to tell a folder from a file drop.
                        std::fs::metadata(&path).ok().filter(|m| m.is_dir())?;
                        Some(path)
                    })
                });
                if let Some(path) = dropped {
                    dialog.root_path = path.display().to_string();
                    dialog.error.clear();
                }
                ui.label(
                    RichText::new("必须是已存在、可写且未被其它工作区使用的文件夹；蛹不会创建目录，也可以把文件夹直接拖进窗口。")
                        .size(11.0)
                        .color(tokens.text_muted),
                );
                ui.add_space(10.0);
                ui.label("图标");
                ui.horizontal(|ui| {
                    for choice in WORKSPACE_ICON_CHOICES {
                        let selected = dialog.icon == *choice;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                        ui.painter().rect(
                            rect,
                            6.0,
                            if selected {
                                tokens.primary_tint
                            } else {
                                Color32::TRANSPARENT
                            },
                            Stroke::new(
                                1.0,
                                if selected {
                                    tokens.primary
                                } else {
                                    tokens.border
                                },
                            ),
                            egui::StrokeKind::Inside,
                        );
                        crate::icons::paint(
                            ui.painter(),
                            rect.shrink(6.0),
                            workspace_icon(choice),
                            if selected {
                                tokens.primary
                            } else {
                                tokens.text_muted
                            },
                        );
                        if response.on_hover_text(*choice).clicked() {
                            dialog.icon = (*choice).to_owned();
                        }
                    }
                });
                ui.add_space(8.0);
                ui.label("颜色");
                ui.horizontal_wrapped(|ui| {
                    for choice in WORKSPACE_COLOR_CHOICES {
                        let selected = dialog.color == *choice;
                        let (rect, response) =
                            ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
                        let center = rect.center();
                        ui.painter()
                            .circle_filled(center, 7.0, theme::workspace_color(choice));
                        if selected {
                            ui.painter()
                                .circle_stroke(center, 10.0, Stroke::new(1.5, tokens.primary));
                        }
                        if response.on_hover_text(*choice).clicked() {
                            dialog.color = (*choice).to_owned();
                        }
                    }
                });
                if !dialog.error.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&dialog.error).size(11.0).color(tokens.danger));
                }
                ui.add_space(12.0);
                let valid = !dialog.name.trim().is_empty() && !dialog.root_path.trim().is_empty();
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // A rejected save keeps the form open with the façade's
                    // message inline, which is what 蛹 does; only a success
                    // closes it.
                    if ui
                        .add_enabled(valid && !dialog.busy, egui::Button::new("保存"))
                        .clicked()
                    {
                        dialog.busy = true;
                        accepted = true;
                    }
                    if ui.add_enabled(!dialog.busy, egui::Button::new("取消")).clicked() {
                        close_requested = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close_requested = true;
                }
            });
        if close_requested {
            open = false;
        }
        if accepted {
            self.submit(BackendCommand::SaveWorkspace {
                workspace_id: dialog.workspace_id.clone(),
                name: dialog.name.trim().to_owned(),
                root_path: dialog.root_path.trim().to_owned(),
                icon: dialog.icon.clone(),
                color: dialog.color.clone(),
            });
            // The dialog stays mounted while the reply is in flight: the
            // catalogue refresh on success is what closes it, and a failure
            // re-uses the same draft instead of losing the user's typing.
        } else if open {
            self.workspace_dialog = Some(dialog);
        }
    }

    /// Deleting a workspace decides what happens to its threads, so the choice
    /// lives in the confirmation instead of a second prompt.
    fn draw_workspace_delete_confirm(&mut self, ctx: &Context, tokens: Tokens) {
        let Some(mut confirm) = self.workspace_delete_confirm.take() else {
            return;
        };
        let mut open = true;
        let mut close_requested = false;
        let mut accepted = false;
        egui::Window::new("删除工作区")
            .id(Id::new("workspace-delete-workspace-dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(RichText::new(format!("确定删除工作区「{}」？", confirm.name)).size(13.0));
                ui.add_space(6.0);
                ui.checkbox(&mut confirm.delete_threads, "同时删除该工作区的线程");
                ui.label(
                    RichText::new(if confirm.delete_threads {
                        "线程记录会被删除，无法恢复。"
                    } else {
                        "未勾选时线程会归档，仍可在归档中找回。"
                    })
                    .size(11.0)
                    .color(tokens.text_muted),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("只解除目录绑定，不会删除磁盘上的文件。")
                        .size(11.0)
                        .color(tokens.text_muted),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let delete = ui.add(
                        egui::Button::new(RichText::new("删除").color(tokens.on_accent))
                            .fill(tokens.danger),
                    );
                    if delete.clicked() {
                        accepted = true;
                    }
                    if ui.button("取消").clicked() {
                        close_requested = true;
                    }
                });
                if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    close_requested = true;
                }
            });
        if close_requested {
            open = false;
        }
        if accepted {
            self.submit(BackendCommand::DeleteWorkspace {
                workspace_id: confirm.workspace_id.clone(),
                delete_threads: confirm.delete_threads,
            });
            self.state.status = "正在删除工作区".to_owned();
        } else if open {
            self.workspace_delete_confirm = Some(confirm);
        }
    }

    fn rename_thread_optimistically(
        &mut self,
        session_id: String,
        original_title: String,
        title: String,
    ) {
        let Some(session) = self
            .state
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        else {
            self.state.status = "线程不存在，无法重命名".to_owned();
            return;
        };
        session.title = title.clone();
        self.pending_thread_renames.push(PendingThreadRename {
            session_id: session_id.clone(),
            original_title,
            requested_title: title.clone(),
        });
        self.state.status = "正在重命名线程".to_owned();
        if let Err(error) = self.backend.submit(BackendCommand::RenameSession {
            session_id: session_id.clone(),
            title: title.clone(),
        }) {
            self.finish_thread_rename(session_id, title, Err(error));
        }
    }

    fn finish_thread_rename(
        &mut self,
        session_id: String,
        title: String,
        result: Result<(), String>,
    ) {
        let Some(index) = self.pending_thread_renames.iter().position(|pending| {
            pending.session_id == session_id && pending.requested_title == title
        }) else {
            return;
        };
        let pending = self.pending_thread_renames.remove(index);
        let has_later_rename = self
            .pending_thread_renames
            .iter()
            .any(|pending| pending.session_id == session_id);
        match result {
            Ok(()) => {
                if !has_later_rename {
                    if let Some(session) = self
                        .state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.title = title;
                    }
                    self.state.status = "线程已重命名".to_owned();
                }
            }
            Err(error) => {
                if !has_later_rename {
                    if let Some(session) = self
                        .state
                        .sessions
                        .iter_mut()
                        .find(|session| session.id == session_id)
                    {
                        session.title = pending.original_title;
                    }
                    self.state.status = format!("无法重命名线程：{error}");
                }
            }
        }
    }

    fn draw_chat(&mut self, ctx: &Context) {
        let tokens = theme::tokens();
        CentralPanel::default()
            .frame(Frame::NONE.fill(tokens.canvas))
            .show(ctx, |ui| {
                // Fixed rectangles keep a growing multiline editor from moving
                // the footer below the viewport. All dimensions are CSS pixels.
                let full = ui.available_rect_before_wrap();
                let ruler_width = if self.state.selected_session.is_some() {
                    18.0
                } else {
                    0.0
                };
                let text_width =
                    (full.width() - 32.0 - ruler_width).clamp(80.0, CHAT_COLUMN_WIDTH) - 30.0;
                let mut job = egui::text::LayoutJob::single_section(
                    self.state.composer.clone(),
                    egui::TextFormat {
                        font_id: egui::FontId::proportional(14.0),
                        color: tokens.text,
                        line_height: Some(20.3),
                        ..Default::default()
                    },
                );
                job.wrap.max_width = text_width;
                let input_height = ui
                    .fonts(|fonts| fonts.layout_job(job).size().y)
                    .clamp(44.0, 168.0);
                // Chips and queued prompts sit above the input card, so the
                // composer reserves their height instead of overlapping the
                // transcript when they appear.
                let strip = composer_strip_height(
                    self.state.attachments.len(),
                    self.state.prompt_queue.len(),
                    text_width + 30.0,
                );
                let composer_height = input_height + 84.0 + strip;
                let split = (full.bottom() - composer_height).max(full.top());
                ui.scope_builder(
                    UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        full.min,
                        egui::pos2(full.right(), split),
                    )),
                    |ui| self.draw_message_body(ui, tokens),
                );
                ui.scope_builder(
                    UiBuilder::new().max_rect(egui::Rect::from_min_max(
                        egui::pos2(full.left(), split),
                        full.max,
                    )),
                    |ui| self.draw_composer(ui, tokens),
                );
            });
    }

    fn draw_message_body(&mut self, ui: &mut Ui, tokens: Tokens) {
        let mut open_prompt_preview = false;
        let mut consumed_scroll = false;
        let mut process_requests: Vec<ProcessPageRequest> = Vec::new();
        let full = ui.max_rect();
        let ruler_width = if self.state.selected_session.is_some() {
            18.0
        } else {
            0.0
        };
        let viewport = egui::Rect::from_min_max(full.min, full.max - Vec2::new(ruler_width, 0.0));
        let output = ui
            .scope_builder(UiBuilder::new().max_rect(viewport), |ui| {
                ScrollArea::vertical()
                    .id_salt("message-list")
                    .auto_shrink([false, false])
                    .min_scrolled_height(0.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .stick_to_bottom(self.follow_tail)
                    .show(ui, |ui| {
                        let content_width = ui.available_width().min(CHAT_COLUMN_WIDTH);
                        let left = (ui.available_width() - content_width).max(0.0) / 2.0;
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add_space(18.0);
                        ui.scope_builder(
                            UiBuilder::new().max_rect(egui::Rect::from_min_size(
                                egui::pos2(ui.min_rect().left() + left, ui.cursor().top()),
                                Vec2::new(content_width, ui.available_height()),
                            )),
                            |ui| {
                                if self.state.turns.is_empty() {
                                    self.draw_empty_state(ui, tokens);
                                } else {
                                    let mut number = 0;
                                    // Paint directly from the borrowed projection.
                                    // Process-page reads are collected and applied
                                    // after the scroll closure so no turn (or the
                                    // whole transcript) is cloned per frame.
                                    for (index, turn) in self.state.turns.iter().enumerate() {
                                        if !turn.user.text.is_empty() {
                                            number += 1;
                                        }
                                        let top_before = ui.cursor().top();
                                        open_prompt_preview |= self.draw_turn(
                                            ui,
                                            turn,
                                            number,
                                            tokens,
                                            &mut process_requests,
                                        );
                                        if self.scroll_to_turn == Some(index) {
                                            // The ruler asked for this turn: put
                                            // its first row under the reader's eye,
                                            // then stop asking on later frames.
                                            let rect = egui::Rect::from_min_max(
                                                egui::pos2(ui.min_rect().left(), top_before),
                                                egui::pos2(
                                                    ui.min_rect().right(),
                                                    ui.cursor().top(),
                                                ),
                                            );
                                            ui.scroll_to_rect(rect, Some(Align::TOP));
                                            consumed_scroll = true;
                                        }
                                    }
                                }
                            },
                        );
                        ui.add_space((full.height() * 0.13).clamp(84.0, 132.0));
                    })
            })
            .inner;
        if consumed_scroll {
            self.follow_tail = false;
            self.scroll_to_turn = None;
        }
        for request in process_requests {
            self.request_process_page_at(&request.turn_id, request.before, request.after);
        }
        if open_prompt_preview {
            self.open_system_prompt_preview();
        }
        // The ruler owns the same separate 18px lane as the web workbench.
        self.draw_turn_ruler(ui, full, tokens);
        // Follow only while the reader is already at the tail. Scrolling up to
        // inspect earlier output must remain stable while native stream deltas
        // continue arriving.
        let distance_to_tail =
            (output.content_size.y - output.inner_rect.height() - output.state.offset.y).max(0.0);
        self.follow_tail = distance_to_tail <= 120.0;
    }

    fn draw_turn_ruler(&mut self, ui: &mut Ui, viewport: egui::Rect, tokens: Tokens) {
        // Turns without user text (the opening system-prompt turn) have no
        // round of their own, so the ruler indexes the remaining rows and maps
        // back to their position in the transcript.
        let rounds: Vec<usize> = self
            .state
            .turns
            .iter()
            .enumerate()
            .filter(|(_, turn)| !turn.user.text.is_empty())
            .map(|(index, _)| index)
            .collect();
        // The web workbench only shows clickable ticks once a thread has more
        // than one round; a single tick would just be noise.
        if !crate::timeline_ruler::visible(rounds.len()) || viewport.height() < 48.0 {
            return;
        }
        let right = viewport.right() - 2.0;
        let top = viewport.top() + 10.0;
        let height = (viewport.height() - 20.0).max(1.0);
        // The projection has no per-round heights, so ticks tile the lane
        // evenly; the content guard only rejects degenerate frames.
        let ticks = crate::timeline_ruler::ticks(rounds.len(), height, top, height, top, height);
        let mut jump = None;
        let mut hovered = None;
        for (order, tick) in ticks.iter().enumerate() {
            let Some(&turn_index) = rounds.get(tick.turn_index) else {
                continue;
            };
            let center_y = top + tick.y + tick.height * 0.5;
            let hit = egui::Rect::from_center_size(
                egui::pos2(right - crate::timeline_ruler::WIDTH * 0.5, center_y),
                egui::vec2(crate::timeline_ruler::WIDTH, tick.height.max(12.0)),
            );
            let response = ui.interact(hit, Id::new(("turn-ruler", order)), Sense::click());
            let is_hovered = response.hovered();
            // Read the click before the response moves into the hover tuple.
            let clicked = response.clicked();
            if is_hovered {
                hovered = Some((turn_index, order + 1, response));
            }
            if clicked {
                jump = Some(turn_index);
            }
            // A longer user message gets a slightly wider tick, which keeps the
            // lane readable without inventing per-round heights.
            let user_len = self.state.turns[turn_index]
                .user
                .text
                .chars()
                .count()
                .min(160) as f32;
            let width = (4.0 + user_len.sqrt() * 0.9).clamp(4.0, 11.0);
            let tick_rect = egui::Rect::from_center_size(
                egui::pos2(right - 3.0 - width * 0.5, center_y),
                egui::vec2(width, if is_hovered { 3.0 } else { 2.0 }),
            );
            ui.painter().rect_filled(
                tick_rect,
                1.0,
                if is_hovered {
                    tokens.primary
                } else {
                    tokens.border_strong
                },
            );
            if is_hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if let Some(index) = jump {
            self.scroll_to_turn = Some(index);
        }
        if let Some((index, number, response)) = hovered {
            let turn = &self.state.turns[index];
            let first_line = turn.user.text.lines().next().unwrap_or_default().trim();
            let title = crate::timeline_ruler::summary_text(number, first_line);
            let answer = turn.assistant.text.trim();
            let preview = if answer.is_empty() {
                "正在生成…".to_owned()
            } else {
                answer.chars().take(120).collect::<String>()
            };
            // Tooltip uses the native popup so the tick lane stays outside the
            // scrolling message layout, like the web preview card.
            response.on_hover_ui(|ui| {
                ui.set_max_width(264.0);
                ui.label(RichText::new(title).size(13.0).color(tokens.text));
                ui.label(
                    RichText::new(preview)
                        .size(12.0)
                        .color(tokens.text_secondary),
                );
            });
        }
    }

    fn draw_empty_state(&self, ui: &mut Ui, tokens: Tokens) {
        ui.add_space(90.0);
        ui.vertical_centered(|ui| {
            let icon = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover()).0;
            ui.painter().rect_filled(icon, 12.0, tokens.primary_tint);
            ui.painter().rect_stroke(
                icon,
                12.0,
                Stroke::new(1.0, tokens.primary_soft),
                egui::epaint::StrokeKind::Inside,
            );
            ui.painter().text(
                icon.center(),
                egui::Align2::CENTER_CENTER,
                "···",
                egui::FontId::proportional(18.0),
                tokens.primary,
            );
            ui.add_space(8.0);
            let (title, subtitle) = match self.state.connection {
                ConnectionState::Starting | ConnectionState::Connecting => {
                    ("正在连接本地工作台", "连接完成后会显示线程和历史消息")
                }
                ConnectionState::Failed => ("工作台暂时不可用", "检查本地运行时状态后重试"),
                _ => (
                    "选择一个线程开始工作",
                    "本地消息、工具过程和流式输出会在这里展开",
                ),
            };
            ui.label(
                RichText::new(title)
                    .size(15.0)
                    .strong()
                    .color(tokens.text_secondary),
            );
            ui.label(RichText::new(subtitle).size(12.0).color(tokens.text_muted));
            if self.state.connection == ConnectionState::Failed && !self.state.status.is_empty() {
                ui.label(
                    RichText::new(&self.state.status)
                        .size(12.0)
                        .color(tokens.danger),
                );
            }
        });
    }

    fn draw_turn(
        &self,
        ui: &mut Ui,
        turn: &TurnView,
        number: usize,
        tokens: Tokens,
        process_requests: &mut Vec<ProcessPageRequest>,
    ) -> bool {
        let mut open_prompt_preview = false;
        if !turn.user.text.is_empty() {
            let row_width = ui.available_width();
            let bubble_max = ((row_width - 48.0) * 0.70).max(48.0);
            let mut job = egui::text::LayoutJob::single_section(
                turn.user.text.clone(),
                egui::TextFormat {
                    font_id: egui::FontId::proportional(14.0),
                    color: tokens.text,
                    line_height: Some(23.8),
                    ..Default::default()
                },
            );
            job.wrap.max_width = bubble_max - 24.0;
            let galley = ui.fonts(|fonts| fonts.layout_job(job));
            let bubble_size = galley.size() + Vec2::new(24.0, 16.0);
            // The web user row also reserves the copy-action line. Keeping
            // its height while hidden avoids reflow when the pointer moves.
            let row = ui
                .allocate_exact_size(Vec2::new(row_width, bubble_size.y + 27.3), Sense::hover())
                .0;
            let bubble = egui::Rect::from_min_size(
                egui::pos2(row.right() - 24.0 - bubble_size.x, row.top()),
                bubble_size,
            );
            theme::paint_user_bubble(ui, bubble);
            // An overlay child must not advance the parent cursor back from
            // the reserved copy-action line into the bubble's text rectangle.
            let mut text_ui =
                ui.new_child(UiBuilder::new().max_rect(bubble.shrink2(Vec2::new(12.0, 8.0))));
            text_ui.add(egui::Label::new(galley).selectable(true));
            let copy_rect = egui::Rect::from_min_size(
                egui::pos2(row.right() - 46.0, bubble.bottom() + 2.0),
                Vec2::splat(22.0),
            );
            let copy = ui.interact(copy_rect, Id::new(("copy-user", &turn.id)), Sense::click());
            if ui.rect_contains_pointer(row) || copy.has_focus() {
                if copy.hovered() {
                    ui.painter().rect_filled(copy_rect, 6.0, tokens.hover);
                }
                crate::icons::paint(
                    ui.painter(),
                    copy_rect.shrink(5.5),
                    crate::icons::Icon::Copy,
                    tokens.text_muted,
                );
            }
            if copy.on_hover_text("复制消息").clicked() {
                ui.ctx().copy_text(turn.user.text.clone());
            }
        }

        let assistant_top = ui.cursor().top();
        self.draw_workflow(ui, turn, tokens, process_requests);
        if !turn.assistant.text.is_empty() {
            // Reserve the paint slot before laying out selectable text so the
            // hover wash is always below the glyphs and selection overlay.
            let background = ui.painter().add(egui::Shape::Noop);
            let body = Frame::NONE
                .inner_margin(Margin {
                    left: 24,
                    right: 24,
                    top: if crate::model::process_panel_visible(turn) {
                        9
                    } else {
                        6
                    },
                    bottom: if crate::model::process_panel_visible(turn) {
                        3
                    } else {
                        6
                    },
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    crate::markdown::show(
                        ui,
                        &turn.assistant.text,
                        tokens.text,
                        crate::model::assistant_streaming(&turn.assistant),
                    );
                });
            if ui.rect_contains_pointer(body.response.rect) {
                ui.painter().set(
                    background,
                    egui::Shape::rect_filled(body.response.rect, 0.0, tokens.reader_hover),
                );
            }
            if number == 0
                && turn.user.text.is_empty()
                && !crate::model::process_panel_visible(turn)
            {
                let center = egui::pos2(body.response.rect.right() - 35.5, assistant_top + 17.0);
                let hit = egui::Rect::from_center_size(center, Vec2::splat(28.0));
                let response =
                    ui.interact(hit, Id::new("thread-system-prompt-preview"), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(hit, 6.0, tokens.hover);
                }
                crate::icons::paint(
                    ui.painter(),
                    egui::Rect::from_center_size(center, Vec2::splat(13.0)),
                    crate::icons::Icon::Eye,
                    tokens.text_muted,
                );
                open_prompt_preview = response.on_hover_text("查看系统提示词").clicked();
            }
        }
        // An initial greeting is plain body text and has no turn footer.
        if crate::model::process_panel_visible(turn) {
            let assistant_rect = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), assistant_top),
                egui::pos2(ui.max_rect().right(), ui.cursor().top() + 22.0),
            );
            self.draw_turn_footer(
                ui,
                turn,
                number,
                ui.rect_contains_pointer(assistant_rect),
                tokens,
            );
            ui.add_space(2.0);
        }
        open_prompt_preview
    }

    fn open_system_prompt_preview(&mut self) {
        let Some(session_id) = self.state.selected_session.clone() else {
            return;
        };
        let request_id = self.state.prompt_preview.request_id.wrapping_add(1);
        self.state.prompt_preview = crate::model::PromptPreview {
            open: true,
            loading: true,
            session_id: Some(session_id.clone()),
            request_id,
            ..Default::default()
        };
        if let Err(error) = self.backend.submit(BackendCommand::PreviewSystemPrompt {
            session_id,
            request_id,
        }) {
            self.state.prompt_preview.loading = false;
            self.state.prompt_preview.error = error;
        }
    }

    fn draw_system_prompt_preview(&mut self, ctx: &Context) {
        if !self.state.prompt_preview.open {
            return;
        }
        if self.state.prompt_preview.session_id != self.state.selected_session {
            self.state.prompt_preview.open = false;
            self.state.prompt_preview.loading = false;
            return;
        }
        let tokens = theme::tokens();
        let screen = ctx.input(|input| input.screen_rect());
        let width = (screen.width() - 64.0).clamp(240.0, 760.0);
        let height = (screen.height() - 96.0).clamp(180.0, 540.0);
        let preview = &mut self.state.prompt_preview;
        let mut close = false;
        let modal = egui::Modal::new(Id::new("thread-system-prompt-window"))
            .frame(
                Frame::NONE
                    .fill(tokens.surface)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(14.0)
                    .inner_margin(Margin::same(20)),
            )
            .backdrop_color(tokens.scrim)
            .show(ctx, |ui| {
                ui.set_width(width - 40.0);
                ui.set_min_height(height - 40.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("系统提示词")
                            .size(16.0)
                            .strong()
                            .color(tokens.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = crate::icons::button(
                            ui,
                            crate::icons::Icon::Close,
                            28.0,
                            tokens.text_muted,
                        )
                        .on_hover_text("关闭")
                        .clicked();
                        if !preview.loading
                            && preview.error.is_empty()
                            && !preview.text.is_empty()
                            && crate::icons::button(
                                ui,
                                crate::icons::Icon::Copy,
                                28.0,
                                tokens.text_muted,
                            )
                            .on_hover_text("复制预览内容")
                            .clicked()
                        {
                            ui.ctx().copy_text(preview.text.clone());
                        }
                    });
                });
                ui.label(
                    RichText::new("当前线程的系统提示词，只读预览")
                        .size(12.0)
                        .color(tokens.text_muted),
                );
                ui.separator();
                if preview.loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("正在读取…");
                    });
                } else if !preview.error.is_empty() {
                    ui.label(
                        RichText::new(&preview.error)
                            .size(13.0)
                            .color(tokens.danger),
                    );
                } else {
                    if preview.truncated {
                        ui.label(
                            RichText::new("内容较长，预览仅显示前 128 KiB。")
                                .size(12.0)
                                .color(tokens.text_muted),
                        );
                    }
                    ScrollArea::vertical()
                        .id_salt(("system-prompt-preview-scroll", preview.request_id))
                        .max_height(height - 132.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&preview.text)
                                        .monospace()
                                        .size(13.0)
                                        .color(tokens.text),
                                )
                                .wrap()
                                .selectable(true),
                            );
                        });
                }
            });
        if close || modal.should_close() {
            preview.open = false;
            preview.loading = false;
            preview.text.clear();
        }
    }

    /// Start one bounded process-page read for a turn.  The loading bit is
    /// stored in the turn projection before submitting the command, so the
    /// immediate-mode list cannot enqueue the same request on every repaint.
    /// Request one side of the durable process keyset. The values are opaque
    /// storage indexes; UI code only ever passes cursors supplied by the last
    /// bounded page.
    fn request_process_page_at(&mut self, turn_id: &str, before: Option<i64>, after: Option<i64>) {
        let Some(session_id) = self.state.selected_session.clone() else {
            return;
        };
        let Some(turn) = self.state.turns.iter_mut().find(|turn| turn.id == turn_id) else {
            return;
        };
        if turn.process_loading {
            return;
        }
        turn.process_loading = true;
        turn.process_error.clear();
        let command = BackendCommand::LoadProcessPage {
            session_id,
            turn_id: turn_id.to_owned(),
            before,
            after,
        };
        if let Err(error) = self.backend.submit(command) {
            turn.process_loading = false;
            turn.process_error = error;
        }
    }

    fn draw_workflow(
        &self,
        ui: &mut Ui,
        turn: &TurnView,
        tokens: Tokens,
        process_requests: &mut Vec<ProcessPageRequest>,
    ) {
        if !crate::model::process_panel_visible(turn) {
            return;
        }
        let id = Id::new(("timeline-process", &turn.id));
        ui.push_id(id, |ui| {
            let mut open = ui
                .data_mut(|data| data.get_temp::<bool>(id))
                .unwrap_or(false);
            // A live turn follows the Slint workbench: the process tail is
            // visible while the model is thinking or executing a tool.
            if matches!(turn.assistant.state.as_str(), "running" | "streaming")
                && !turn.workflow.is_empty()
            {
                open = true;
                ui.data_mut(|data| data.insert_temp(id, true));
            }
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
            if response.clicked() {
                open = !open;
                ui.data_mut(|data| data.insert_temp(id, open));
                if open {
                    process_requests.push(ProcessPageRequest {
                        turn_id: turn.id.clone(),
                        before: None,
                        after: None,
                    });
                }
            }
            let disclosure =
                egui::Rect::from_min_size(rect.min + Vec2::new(25.0, 8.0), Vec2::splat(24.0));
            ui.painter().rect_filled(
                disclosure.translate(Vec2::new(0.0, 1.0)),
                10.0,
                tokens.shadow_lift,
            );
            ui.painter().rect_filled(disclosure, 10.0, tokens.surface);
            ui.painter().rect_stroke(
                disclosure,
                10.0,
                Stroke::new(
                    1.0,
                    if response.hovered() {
                        tokens.border_strong
                    } else {
                        tokens.border
                    },
                ),
                egui::StrokeKind::Inside,
            );
            // Font Awesome's caret silhouette is a filled triangle on the web.
            let center = disclosure.center();
            let points = if open {
                vec![
                    center + Vec2::new(-4.5, -2.0),
                    center + Vec2::new(4.5, -2.0),
                    center + Vec2::new(0.0, 3.0),
                ]
            } else {
                vec![
                    center + Vec2::new(-2.0, -4.5),
                    center + Vec2::new(3.0, 0.0),
                    center + Vec2::new(-2.0, 4.5),
                ]
            };
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                tokens.text_muted,
                Stroke::NONE,
            ));
            let title = ui.painter().layout_no_wrap(
                "工作过程".to_owned(),
                egui::FontId::proportional(14.0),
                tokens.text_secondary,
            );
            let title_pos = rect.min + Vec2::new(58.0, (40.0 - title.size().y) / 2.0);
            let count_x = title_pos.x + title.size().x + 8.0;
            ui.painter().galley(title_pos, title, tokens.text_secondary);
            let count = ui.painter().layout_no_wrap(
                format!("{} 步", turn.process_steps.max(turn.workflow.len())),
                egui::FontId::proportional(13.0),
                tokens.text_muted,
            );
            let latest_x = count_x + count.size().x + 8.0;
            ui.painter().galley(
                egui::pos2(count_x, rect.center().y - count.size().y / 2.0),
                count,
                tokens.text_muted,
            );
            let latest = match turn.assistant.state.as_str() {
                "running" | "streaming" => "正在处理…",
                "error" | "failed" => "执行失败",
                _ => "已完成",
            };
            ui.painter().text(
                egui::pos2(latest_x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                latest,
                egui::FontId::proportional(13.0),
                tokens.text_muted,
            );
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if !open {
                return;
            }
            if !turn.process_loaded && !turn.process_loading && turn.process_error.is_empty() {
                process_requests.push(ProcessPageRequest {
                    turn_id: turn.id.clone(),
                    before: None,
                    after: None,
                });
            }
            let max_height = (ui.ctx().screen_rect().height() * 0.48).min(420.0);
            ScrollArea::vertical()
                .id_salt("process-body")
                .max_height(max_height)
                .min_scrolled_height(0.0)
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if turn.process_loading {
                        ui.horizontal(|ui| {
                            ui.add_space(56.0);
                            ui.spinner();
                            ui.label(
                                RichText::new("正在读取工作过程…")
                                    .size(12.0)
                                    .color(tokens.text_muted),
                            );
                        });
                    } else if !turn.process_error.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(56.0);
                            ui.label(
                                RichText::new(&turn.process_error)
                                    .size(12.0)
                                    .color(tokens.danger),
                            );
                            if ui.small_button("重试").clicked() {
                                process_requests.push(ProcessPageRequest {
                                    turn_id: turn.id.clone(),
                                    before: None,
                                    after: None,
                                });
                            }
                        });
                    } else if turn.workflow.is_empty() {
                        ui.horizontal(|ui| {
                            ui.add_space(56.0);
                            ui.label(
                                RichText::new("暂无已记录的工作过程")
                                    .size(12.0)
                                    .color(tokens.text_muted),
                            );
                        });
                    }
                    if turn.process_has_older {
                        let older = ui
                            .add(egui::Button::new(
                                RichText::new("查看更早的过程")
                                    .size(12.0)
                                    .color(tokens.text_secondary),
                            ))
                            .on_hover_text("读取上一页最多 100 个过程项");
                        if older.clicked() {
                            process_requests.push(ProcessPageRequest {
                                turn_id: turn.id.clone(),
                                before: turn.process_first,
                                after: None,
                            });
                        }
                    }
                    for item in &turn.workflow {
                        let item_id = Id::new(("timeline-process-item", &turn.id, &item.id));
                        let mut item_open = ui
                            .data_mut(|data| data.get_temp::<bool>(item_id))
                            .unwrap_or(false);
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 26.0),
                            Sense::click(),
                        );
                        if response.clicked() {
                            item_open = !item_open;
                            ui.data_mut(|data| data.insert_temp(item_id, item_open));
                        }
                        let center = egui::pos2(row.left() + 34.0, row.center().y);
                        ui.painter().line_segment(
                            [
                                egui::pos2(center.x, row.top()),
                                egui::pos2(center.x, row.bottom()),
                            ],
                            Stroke::new(1.0, tokens.timeline_line),
                        );
                        let failed = matches!(item.state.as_str(), "error" | "failed");
                        let running = matches!(item.state.as_str(), "running" | "streaming");
                        let color = if failed { tokens.danger } else { tokens.process };
                        if running {
                            egui::Spinner::new().color(color).paint_at(
                                ui,
                                egui::Rect::from_center_size(center, Vec2::splat(12.0)),
                            );
                        } else {
                            ui.painter().circle_filled(center, 6.0, tokens.canvas);
                            ui.painter()
                                .circle_stroke(center, 5.5, Stroke::new(1.0, color));
                            crate::icons::paint(
                                ui.painter(),
                                egui::Rect::from_center_size(center, Vec2::splat(7.0)),
                                if failed {
                                    crate::icons::Icon::Close
                                } else {
                                    crate::icons::Icon::Check
                                },
                                color,
                            );
                        }
                        let is_tool = item.kind == crate::model::WorkflowKind::Tool;
                        let icon = if is_tool {
                            match item.tool_name.as_str() {
                                "read_file" | "读取文件" | "write_file" | "写入文件" | "edit_file" | "apply_patch" | "应用补丁" => Some(crate::icons::Icon::File),
                                "list_files" | "list_file" | "列出文件" => Some(crate::icons::Icon::FolderOpen),
                                "execute_command" | "run_command" | "shell" | "运行命令" | "command_session" => Some(crate::icons::Icon::Terminal),
                                "web_fetch" | "web_search" | "网页抓取" | "网页搜索" => Some(crate::icons::Icon::Link),
                                _ => None,
                            }
                        } else { None };
                        let mut title_x = row.left() + 52.0;
                        if let Some(icon) = icon {
                            crate::icons::paint(ui.painter(), egui::Rect::from_center_size(
                                egui::pos2(title_x + 6.5, row.center().y), Vec2::splat(13.0)), icon, tokens.text_muted);
                            title_x += 21.0;
                        }
                        let title = ui.painter().layout_no_wrap(
                            item.title.clone(), egui::FontId::proportional(13.0), tokens.text_muted,
                        );
                        let title_width = title.size().x.min((row.width() * 0.4).max(40.0));
                        let title_pos = egui::pos2(title_x, row.center().y - title.size().y / 2.0);
                        ui.painter().with_clip_rect(egui::Rect::from_min_size(title_pos, Vec2::new(title_width, 26.0)))
                            .galley(title_pos, title, tokens.text_muted);
                        let mut summary_x = title_pos.x + title_width + 8.0;
                        if is_tool {
                            for (target_index, target) in item.targets.iter().take(4).enumerate() {
                                let target_text = ui.painter().layout_no_wrap(target.clone(), egui::FontId::proportional(12.0), tokens.text_muted);
                                let chip_width = (target_text.size().x + 14.0).min(180.0);
                                if summary_x + chip_width > row.right() - 24.0 { break; }
                                let chip = egui::Rect::from_center_size(
                                    egui::pos2(summary_x + chip_width / 2.0, row.center().y), Vec2::new(chip_width, 18.0),
                                );
                                ui.painter().rect_filled(chip, 6.0, tokens.hover);
                                ui.painter().with_clip_rect(chip.shrink2(Vec2::new(7.0, 0.0))).galley(
                                    egui::pos2(chip.left() + 7.0, chip.center().y - target_text.size().y / 2.0), target_text, tokens.text_muted,
                                );
                                ui.interact(chip, item_id.with(("target", target_index)), Sense::hover()).on_hover_text(target);
                                summary_x += chip_width + 6.0;
                            }
                        }
                        // An argument already shown in a target chip does not
                        // appear again as the plain one-line summary.
                        let summary = if item.targets.iter().any(|target| target == &item.preview) { "" } else { &item.preview };
                        if summary_x < row.right() - 24.0 {
                            let mut summary_ui = ui.new_child(
                                UiBuilder::new().max_rect(egui::Rect::from_min_max(
                                    egui::pos2(summary_x, row.top()), egui::pos2(row.right() - 24.0, row.bottom()),
                                )).layout(Layout::left_to_right(Align::Center)),
                            );
                            summary_ui.add(egui::Label::new(RichText::new(summary)
                                .size(12.0)
                                .color(tokens.process)).truncate());
                        }
                        response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if item_open {
                            Frame::NONE
                                .inner_margin(Margin {
                                    left: 54,
                                    right: 24,
                                    top: 12,
                                    bottom: 20,
                                })
                                .show(ui, |ui| {
                                    ui.scope(|ui| {
                                            ui.set_width(ui.available_width());
                                            ScrollArea::vertical().id_salt(item_id).max_height(240.0)
                                                .min_scrolled_height(0.0).show(ui, |ui| {
                                                    if item.detail.is_empty() {
                                                        ui.label(RichText::new("暂无已加载详情").size(12.0).color(tokens.text_muted));
                                                    } else {
                                                        let text = egui::text::LayoutJob::single_section(item.detail.clone(), egui::TextFormat {
                                                            font_id: egui::FontId::proportional(12.0),
                                                            color: tokens.text,
                                                            line_height: Some(18.0),
                                                            ..Default::default()
                                                        });
                                                        ui.add(egui::Label::new(text).selectable(true).wrap());
                                                        if item.detail_truncated {
                                                            ui.add_space(4.0);
                                                            ui.label(RichText::new("仅显示部分详情，完整结果保留在本地记录中").size(11.0).color(tokens.text_muted));
                                                        }
                                                    }
                                                });
                                        });
                                });
                        }
                    }
                    if turn.process_has_newer {
                        let newer = ui
                            .add(egui::Button::new(
                                RichText::new("查看后续过程")
                                    .size(12.0)
                                    .color(tokens.text_secondary),
                            ))
                            .on_hover_text("读取下一页最多 100 个过程项");
                        if newer.clicked() {
                            process_requests.push(ProcessPageRequest {
                                turn_id: turn.id.clone(),
                                before: None,
                                after: turn.process_last,
                            });
                        }
                    }
                });
        });
    }

    fn draw_turn_footer(
        &self,
        ui: &mut Ui,
        turn: &TurnView,
        number: usize,
        assistant_hovered: bool,
        tokens: Tokens,
    ) {
        let rect = ui
            .allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover())
            .0;
        let copy_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 46.0, rect.top()),
            Vec2::splat(22.0),
        );
        let session =
            self.state.sessions.iter().find(|session| {
                Some(session.id.as_str()) == self.state.selected_session.as_deref()
            });
        // The footer is a projection: `timeline_footer` decides which facts are
        // known well enough to print, this function only lays them out.
        let facts =
            crate::timeline_footer::facts_from_turn(turn, number.saturating_sub(1), session);
        let segments = crate::timeline_footer::segments(&facts);
        let limit = copy_rect.left() - 12.0;
        let mut x = rect.left() + 24.0;
        for (index, segment) in segments.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(
                segment.text.clone(),
                egui::FontId::proportional(11.0),
                tokens.text_muted,
            );
            // The first segment carries the tinted chip, the rest stay plain so
            // a long footer does not turn into a row of buttons.
            let padded = index == 0;
            let chip_width = galley.size().x + if padded { 16.0 } else { 0.0 };
            if x + chip_width > limit {
                break;
            }
            let chip = egui::Rect::from_min_size(
                egui::pos2(x, rect.top() + 1.0),
                Vec2::new(chip_width, 20.0),
            );
            if padded {
                ui.painter().rect_filled(chip, 9.0, tokens.success_tint);
                ui.painter().rect_stroke(
                    chip,
                    9.0,
                    Stroke::new(1.0, tokens.success_border),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().galley(
                egui::pos2(
                    chip.left() + if padded { 8.0 } else { 0.0 },
                    chip.center().y - galley.size().y / 2.0,
                ),
                galley,
                tokens.text_muted,
            );
            let hit = ui.interact(
                chip,
                Id::new(("turn-footer", &turn.id, index)),
                Sense::hover(),
            );
            if !segment.tooltip.is_empty() {
                hit.on_hover_text(&segment.tooltip);
            }
            x = chip.right();
            if index + 1 < segments.len() {
                x += 4.0;
                ui.painter().text(
                    egui::pos2(x, rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "·",
                    egui::FontId::proportional(11.0),
                    tokens.text_muted,
                );
                x += 8.0;
            }
        }
        let copy = ui.interact(
            copy_rect,
            Id::new(("copy-assistant", &turn.id)),
            Sense::click(),
        );
        if assistant_hovered || copy.has_focus() {
            if copy.hovered() {
                ui.painter().rect_filled(copy_rect, 6.0, tokens.hover);
            }
            crate::icons::paint(
                ui.painter(),
                copy_rect.shrink(5.5),
                crate::icons::Icon::Copy,
                tokens.text_muted,
            );
        }
        if copy.on_hover_text("复制消息").clicked() {
            ui.ctx().copy_text(turn.assistant.text.clone());
        }
    }

    /// Claim files dropped on the window as composer attachments.
    ///
    /// The drop is a window-level event, not a widget one, so this runs once
    /// per frame from the composer and empties the raw list afterwards; leaving
    /// it in `raw` would add the same file again on the next frame.
    fn accept_dropped_files(&mut self, ui: &mut Ui) {
        let dropped = ui
            .ctx()
            .input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        if dropped.is_empty() {
            return;
        }
        if self.state.selected_session.is_none() {
            self.state.composer_notice = "请先选择一个线程再添加附件".to_owned();
            return;
        }
        let mut added = 0usize;
        for file in dropped {
            let Some(path) = file.path else {
                self.state.composer_notice = "只能添加本地文件".to_owned();
                continue;
            };
            let path = path.to_string_lossy().into_owned();
            let name = if file.name.trim().is_empty() {
                std::path::Path::new(&path)
                    .file_name()
                    .map(|value| value.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone())
            } else {
                file.name.clone()
            };
            // A dropped entry may be a directory; `metadata` is the only way to
            // tell without reading the whole file into memory.
            let attachment = match std::fs::metadata(&path) {
                Ok(metadata) if metadata.is_dir() => {
                    crate::composer_attachments::Attachment::directory(path, name)
                }
                Ok(metadata) => {
                    crate::composer_attachments::Attachment::new(path, name, metadata.len())
                }
                Err(_) => crate::composer_attachments::Attachment::new(
                    path,
                    name,
                    file.bytes
                        .as_ref()
                        .map(|bytes| bytes.len() as u64)
                        .unwrap_or(0),
                ),
            };
            match self.state.attachments.add(attachment) {
                Ok(()) => added += 1,
                Err(error) => self.state.composer_notice = error.message().to_owned(),
            }
        }
        if added > 0 {
            self.state.composer_notice = format!("已添加 {added} 个附件");
        }
    }

    /// Chips and queued prompts above the input card.
    ///
    /// Returns intents instead of mutating: the caller owns the draft, the
    /// queue and the attachment set, and this runs inside a borrowed `ui`.
    fn draw_composer_strip(
        &self,
        ui: &mut Ui,
        strip: egui::Rect,
        tokens: Tokens,
    ) -> ComposerStripAction {
        let mut action = ComposerStripAction::default();
        let queued = self.state.prompt_queue.items();
        for (index, text) in queued.iter().enumerate() {
            let row = egui::Rect::from_min_size(
                egui::pos2(
                    strip.left(),
                    strip.top() + index as f32 * COMPOSER_QUEUE_ROW_HEIGHT,
                ),
                Vec2::new(strip.width(), COMPOSER_QUEUE_ROW_HEIGHT - 4.0),
            );
            let response = ui.interact(row, Id::new(("composer-queue-row", index)), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(row, 6.0, tokens.hover);
            }
            let badge = egui::Rect::from_center_size(
                egui::pos2(row.left() + 10.0, row.center().y),
                Vec2::new(18.0, 16.0),
            );
            ui.painter().rect_filled(badge, 5.0, tokens.chip);
            ui.painter().text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}", index + 1),
                egui::FontId::proportional(10.0),
                tokens.text_muted,
            );
            let remove = egui::Rect::from_center_size(
                egui::pos2(row.right() - 10.0, row.center().y),
                Vec2::splat(16.0),
            );
            let hint_width = if index == 0 { 42.0 } else { 0.0 };
            let text_rect = egui::Rect::from_min_max(
                egui::pos2(badge.right() + 6.0, row.top()),
                egui::pos2(remove.left() - 4.0 - hint_width, row.bottom()),
            );
            if text_rect.width() > 4.0 {
                let galley = ui.painter().layout(
                    text.clone(),
                    egui::FontId::proportional(12.0),
                    tokens.text_secondary,
                    text_rect.width(),
                );
                ui.painter().with_clip_rect(text_rect).galley(
                    text_rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
                    galley,
                    tokens.text_secondary,
                );
            }
            if index == 0 {
                ui.painter().text(
                    egui::pos2(remove.left() - 6.0, row.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "待发送",
                    egui::FontId::proportional(10.0),
                    tokens.text_muted,
                );
            }
            let remove_response = ui.interact(
                remove,
                Id::new(("composer-queue-remove", index)),
                Sense::click(),
            );
            if remove_response.hovered() {
                ui.painter().rect_filled(remove, 4.0, tokens.chip);
            }
            crate::icons::paint(
                ui.painter(),
                remove.shrink(4.0),
                crate::icons::Icon::Close,
                tokens.text_muted,
            );
            if remove_response.clicked() {
                action.remove_queued = Some(index);
            } else if response.clicked() {
                // Clicking the row takes it back into the draft for editing
                // instead of forcing the user to retype it.
                action.recall_queued = Some(index);
            }
        }

        let per_row = ((strip.width() + COMPOSER_CHIP_GAP)
            / (COMPOSER_CHIP_WIDTH + COMPOSER_CHIP_GAP))
            .floor()
            .max(1.0) as usize;
        let chip_top = strip.top() + queued.len() as f32 * COMPOSER_QUEUE_ROW_HEIGHT;
        let chip_width = COMPOSER_CHIP_WIDTH.min(strip.width());
        for (index, chip) in self.state.attachments.chips().iter().enumerate() {
            let row_index = index / per_row;
            let column = index % per_row;
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    strip.left() + column as f32 * (COMPOSER_CHIP_WIDTH + COMPOSER_CHIP_GAP),
                    chip_top + row_index as f32 * COMPOSER_CHIP_ROW_HEIGHT,
                ),
                Vec2::new(chip_width, COMPOSER_CHIP_HEIGHT),
            );
            if rect.right() > strip.right() + 0.5 {
                break;
            }
            ui.painter().rect_filled(rect, 8.0, tokens.primary_tint);
            ui.painter().rect_stroke(
                rect,
                8.0,
                Stroke::new(1.0, tokens.border),
                egui::StrokeKind::Inside,
            );
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(rect.left() + 13.0, rect.center().y),
                    Vec2::splat(12.0),
                ),
                crate::icons::Icon::Paperclip,
                tokens.text_muted,
            );
            let remove = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 12.0, rect.center().y),
                Vec2::splat(18.0),
            );
            let size_galley = ui.painter().layout_no_wrap(
                chip.size_text.clone(),
                egui::FontId::proportional(10.0),
                tokens.text_muted,
            );
            let size_width = size_galley.size().x.min(56.0);
            let name_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 24.0, rect.top()),
                egui::pos2(remove.left() - 4.0 - size_width - 4.0, rect.bottom()),
            );
            if name_rect.width() > 4.0 {
                let galley = ui.painter().layout(
                    chip.name.clone(),
                    egui::FontId::proportional(12.0),
                    tokens.text,
                    name_rect.width(),
                );
                ui.painter().with_clip_rect(name_rect).galley(
                    name_rect.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
                    galley,
                    tokens.text,
                );
            }
            ui.painter()
                .with_clip_rect(egui::Rect::from_min_max(
                    egui::pos2(remove.left() - size_width - 2.0, rect.top()),
                    egui::pos2(remove.left() - 2.0, rect.bottom()),
                ))
                .galley(
                    egui::pos2(
                        remove.left() - size_width - 2.0,
                        rect.center().y - size_galley.size().y / 2.0,
                    ),
                    size_galley,
                    tokens.text_muted,
                );
            let remove_response = ui.interact(
                remove,
                Id::new(("composer-chip-remove", &chip.path)),
                Sense::click(),
            );
            if remove_response.hovered() {
                ui.painter().rect_filled(remove, 4.0, tokens.card_border);
            }
            crate::icons::paint(
                ui.painter(),
                remove.shrink(5.0),
                crate::icons::Icon::Close,
                tokens.text_secondary,
            );
            let clicked = remove_response.clicked();
            remove_response
                .on_hover_text("移除附件")
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if clicked {
                action.remove_attachment = Some(chip.path.clone());
            }
        }
        action
    }

    fn draw_composer(&mut self, ui: &mut Ui, tokens: Tokens) {
        use crate::icons::{self, Icon};
        self.accept_dropped_files(ui);
        let full = ui.max_rect();
        let ruler_width = if self.state.selected_session.is_some() {
            18.0
        } else {
            0.0
        };
        let available_width = (full.width() - 32.0 - ruler_width).max(120.0);
        let width = available_width.min(CHAT_COLUMN_WIDTH);
        let left = full.left() + 16.0 + (available_width - width) / 2.0;
        let strip_height = composer_strip_height(
            self.state.attachments.len(),
            self.state.prompt_queue.len(),
            width,
        );
        let strip = egui::Rect::from_min_size(
            egui::pos2(left, full.top() + 8.0),
            Vec2::new(width, strip_height.max(0.0)),
        );
        let card = egui::Rect::from_min_size(
            egui::pos2(left, strip.bottom()),
            Vec2::new(width, (full.height() - 58.0 - strip_height).max(70.0)),
        );
        ui.painter().rect_filled(
            card.translate(Vec2::new(0.0, 1.0)),
            16.0,
            tokens.shadow_rest,
        );
        ui.painter().rect_filled(card, 16.0, tokens.surface);
        ui.painter().rect_stroke(
            card,
            16.0,
            Stroke::new(1.0, tokens.border),
            egui::StrokeKind::Inside,
        );
        // Notices are transient: the clock restarts whenever the text changes,
        // so a repeat of the same message still refreshes it.
        let now = ui.ctx().input(|input| input.time);
        if self.state.composer_notice != self.composer_notice_seen {
            self.composer_notice_seen
                .clone_from(&self.state.composer_notice);
            self.composer_notice_at = now;
        }
        let notice = (!self.state.composer_notice.is_empty()
            && now - self.composer_notice_at < COMPOSER_NOTICE_SECONDS)
            .then(|| self.state.composer_notice.clone());
        if notice.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(400));
        }
        if strip_height > 0.0 {
            let strip_action = self.draw_composer_strip(ui, strip, tokens);
            if let Some(path) = strip_action.remove_attachment {
                if self.state.attachments.remove(&path) {
                    self.state.composer_notice = format!("已移除 {path}");
                }
            }
            if let Some(index) = strip_action.remove_queued {
                if self.state.prompt_queue.remove(index) {
                    self.state.composer_notice = "已移除待发送消息".to_owned();
                }
            }
            if let Some(index) = strip_action.recall_queued {
                // Recall puts the text back in the draft and drops it from the
                // queue; leaving both copies would send it twice.
                if let Some(text) = self.state.prompt_queue.items().get(index).cloned() {
                    if self.state.prompt_queue.remove(index) {
                        if self.state.composer.is_empty() {
                            self.state.composer = text;
                        } else {
                            self.state.composer.push('\n');
                            self.state.composer.push_str(&text);
                        }
                        self.state.composer_notice = "已取回待发送消息".to_owned();
                    }
                }
            }
        }
        // Hovering files over the window is the only affordance that tells the
        // user a drop will attach rather than navigate.
        let hovering_files = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
        if hovering_files {
            ui.painter().rect_stroke(
                card.shrink(1.0),
                16.0,
                Stroke::new(1.0, tokens.primary),
                egui::StrokeKind::Inside,
            );
            let hint = egui::Rect::from_center_size(card.center(), Vec2::new(240.0, 26.0));
            ui.painter().rect_filled(hint, 8.0, tokens.primary_tint);
            ui.painter().text(
                hint.center(),
                egui::Align2::CENTER_CENTER,
                "松开以添加附件",
                egui::FontId::proportional(12.0),
                tokens.primary,
            );
        }
        let mut send_now = false;
        let mut stop_now = false;
        let editor_id = Id::new("chat-composer-input");
        let enter_sends = self.state.settings_send_key == "Enter 发送";
        let ctrl_enter_sends = self.state.settings_send_key == "Ctrl+Enter 发送";
        // Consume the sending shortcut before TextEdit handles it. The editor
        // keeps focus after sending, and Shift+Enter always inserts a newline.
        let ime_id = editor_id.with("composition");
        let mut composing = ui
            .data_mut(|data| data.get_temp::<bool>(ime_id))
            .unwrap_or(false);
        let mut composition_event = composing;
        ui.input(|input| {
            for event in &input.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                        composing = !text.is_empty();
                        composition_event = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                        composing = false;
                        composition_event = true;
                    }
                    egui::Event::Ime(egui::ImeEvent::Disabled) => composing = false,
                    _ => {}
                }
            }
        });
        ui.data_mut(|data| data.insert_temp(ime_id, composing));
        if ui.memory(|memory| memory.has_focus(editor_id))
            && !self.state.settings_open
            && !composition_event
        {
            let expected = if enter_sends {
                Some(egui::Modifiers::NONE)
            } else if ctrl_enter_sends {
                Some(egui::Modifiers::CTRL)
            } else {
                None
            };
            if let Some(expected) = expected {
                // Logical shortcut matching accepts extra Shift. Match exact
                // modifiers so Shift+Enter remains available for newlines.
                ui.input_mut(|input| input.events.retain(|event| {
                    let matches = matches!(event, egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. }
                        if modifiers.matches_exact(expected));
                    send_now |= matches;
                    !matches
                }));
            }
        }
        // Up/Down recall earlier prompts. Only while the draft is a single
        // line: inside a multi-line draft the arrows must move the caret, not
        // replace the text the user is editing.
        let mut recall_up = false;
        let mut recall_down = false;
        if ui.memory(|memory| memory.has_focus(editor_id))
            && !self.state.settings_open
            && !composition_event
            && !self.state.composer.contains('\n')
        {
            ui.input_mut(|input| {
                input.events.retain(|event| match event {
                    egui::Event::Key {
                        key: egui::Key::ArrowUp,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.matches_exact(egui::Modifiers::NONE) => {
                        recall_up = true;
                        false
                    }
                    egui::Event::Key {
                        key: egui::Key::ArrowDown,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.matches_exact(egui::Modifiers::NONE) => {
                        recall_down = true;
                        false
                    }
                    _ => true,
                });
            });
        }
        if recall_up {
            let current = self.state.composer.clone();
            if let Some(text) = self.state.prompt_history.previous(&current) {
                self.state.composer = text;
            }
        }
        if recall_down {
            // `None` means "past the newest entry": restore the draft the user
            // had before browsing started.
            match self.state.prompt_history.next() {
                Some(text) => self.state.composer = text,
                None => self.state.composer = self.state.prompt_history.draft().to_owned(),
            }
        }
        let hint = match self.state.settings_send_key.as_str() {
            "Ctrl+Enter 发送" => "输入目标，Ctrl+Enter 发送",
            "不使用快捷键" => "输入目标",
            _ => "输入目标，Enter 发送，Shift+Enter 换行",
        };
        let text_rect = card.shrink2(Vec2::new(15.0, 13.0));
        ui.scope_builder(UiBuilder::new().max_rect(text_rect), |ui| {
            ScrollArea::vertical()
                .id_salt("composer-text-scroll")
                .max_height(text_rect.height())
                .min_scrolled_height(0.0)
                .auto_shrink([false, false])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    let mut editor = TextEdit::multiline(&mut self.state.composer)
                        .id(editor_id)
                        .frame(false)
                        .margin(Margin::ZERO)
                        .font(egui::FontId::proportional(14.0))
                        .desired_width(text_rect.width())
                        .desired_rows(2)
                        .hint_text(RichText::new(hint).color(tokens.text_muted));
                    if enter_sends {
                        editor = editor.return_key(egui::KeyboardShortcut::new(
                            egui::Modifiers::SHIFT,
                            egui::Key::Enter,
                        ));
                    }
                    ui.add_sized(Vec2::new(text_rect.width(), text_rect.height()), editor);
                });
        });
        let actions = egui::Rect::from_min_size(
            egui::pos2(left, card.bottom() + 8.0),
            Vec2::new(width, 30.0),
        );
        let plus_rect = egui::Rect::from_center_size(
            egui::pos2(actions.left() + 14.0, actions.center().y),
            Vec2::splat(24.0),
        );
        let plus = ui.interact(plus_rect, Id::new("composer-plus"), Sense::click());
        ui.painter().rect_filled(
            plus_rect,
            8.0,
            if plus.hovered() {
                tokens.hover
            } else {
                tokens.surface
            },
        );
        ui.painter().rect_stroke(
            plus_rect,
            8.0,
            Stroke::new(1.0, tokens.border_strong),
            egui::StrokeKind::Inside,
        );
        icons::paint(
            ui.painter(),
            plus_rect.shrink(8.0),
            Icon::Plus,
            tokens.text_secondary,
        );
        let plus_clicked = plus.clicked();
        if plus_clicked {
            self.composer_menu_open = !self.composer_menu_open;
        }
        plus.on_hover_text("更多操作")
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        let folder_rect = egui::Rect::from_center_size(
            egui::pos2(plus_rect.right() + 13.0, actions.center().y),
            Vec2::splat(14.0),
        );
        icons::paint(ui.painter(), folder_rect, Icon::FolderOpen, tokens.primary);
        let workspace = self
            .state
            .selected()
            .map(|session| session.workspace.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("本地工作区");
        let directory_width = if width < 520.0 {
            88.0
        } else if width < 720.0 {
            128.0
        } else {
            200.0
        };
        let directory_rect = egui::Rect::from_min_size(
            egui::pos2(folder_rect.right() + 5.0, actions.top()),
            Vec2::new(directory_width, 30.0),
        );
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(directory_rect)
                .layout(Layout::left_to_right(Align::Center)),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(workspace)
                            .size(12.0)
                            .color(tokens.text_secondary),
                    )
                    .truncate(),
                );
            },
        );

        let action_rect = egui::Rect::from_center_size(
            egui::pos2(actions.right() - 14.0, actions.center().y),
            Vec2::splat(28.0),
        );
        // An attachment-only prompt is a real send: the desktop flow lets a
        // dropped document speak for itself, so the draft may be empty.
        let has_payload =
            !self.state.composer.trim().is_empty() || !self.state.attachments.is_empty();
        let can_send = self.state.selected_session.is_some() && has_payload && !self.state.busy;
        let action_enabled = self.state.busy || can_send;
        let action = ui.interact(
            action_rect,
            Id::new("composer-send-stop"),
            if action_enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        ui.painter().rect_filled(
            action_rect,
            8.0,
            if !action_enabled {
                tokens.border_strong
            } else if action.hovered() {
                tokens.primary_hover
            } else {
                tokens.primary
            },
        );
        icons::paint(
            ui.painter(),
            action_rect.shrink(7.0),
            if self.state.busy {
                Icon::Stop
            } else {
                Icon::ArrowUp
            },
            tokens.surface,
        );
        if action.clicked() {
            if self.state.busy {
                stop_now = true;
            } else {
                send_now = true;
            }
        }
        action.on_hover_text(if self.state.busy {
            "停止生成"
        } else {
            "发送消息"
        });
        let model_rect = egui::Rect::from_min_max(
            egui::pos2(action_rect.left() - 120.0, actions.top()),
            egui::pos2(action_rect.left() - 6.0, actions.bottom()),
        );
        // The notice shares the action row: between the workspace label and the
        // model picker there is room for one elided line, and it is the only
        // place that never moves the send button.
        if let Some(notice) = &notice {
            let region = egui::Rect::from_min_max(
                egui::pos2(directory_rect.right() + 8.0, actions.top()),
                egui::pos2(model_rect.left() - 8.0, actions.bottom()),
            );
            if region.width() > 60.0 {
                let galley = ui.painter().layout(
                    notice.clone(),
                    egui::FontId::proportional(11.0),
                    tokens.text_muted,
                    region.width(),
                );
                ui.painter().with_clip_rect(region).galley(
                    region.left_center() - Vec2::new(0.0, galley.size().y / 2.0),
                    galley,
                    tokens.text_muted,
                );
            }
        }
        let model = ui.interact(model_rect, Id::new("composer-model"), Sense::click());
        if model.hovered() {
            ui.painter().rect_filled(model_rect, 8.0, tokens.hover);
        }
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(model_rect.left() + 16.0, model_rect.center().y),
                Vec2::splat(16.0),
            ),
            Icon::Brain,
            tokens.text_muted,
        );
        if self.state.model_picker_saving {
            ui.painter().circle_stroke(
                egui::pos2(model_rect.left() + 16.0, model_rect.center().y),
                7.0,
                Stroke::new(1.5, tokens.primary),
            );
        }
        let model_label = self.current_model_label();
        let model_text_rect = egui::Rect::from_min_max(
            egui::pos2(model_rect.left() + 30.0, model_rect.top()),
            egui::pos2(model_rect.right() - 22.0, model_rect.bottom()),
        );
        let model_galley = ui.painter().layout_no_wrap(
            model_label.to_owned(),
            egui::FontId::proportional(13.0),
            tokens.text_secondary,
        );
        ui.painter().with_clip_rect(model_text_rect).galley(
            model_text_rect.left_center() - Vec2::new(0.0, model_galley.size().y / 2.0),
            model_galley,
            tokens.text_secondary,
        );
        icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(model_rect.right() - 12.0, model_rect.center().y),
                Vec2::splat(10.0),
            ),
            Icon::ChevronDown,
            tokens.text_muted,
        );
        let model_clicked = model.clicked();
        if model_clicked {
            self.state.model_picker_open = !self.state.model_picker_open;
            if self.state.model_picker_open
                && self.state.models.is_empty()
                && !self.state.model_picker_loading
            {
                self.state.model_picker_loading = true;
                self.state.model_picker_error.clear();
                if let Err(error) = self.backend.submit(BackendCommand::LoadModels) {
                    self.state.model_picker_loading = false;
                    self.state.model_picker_error = error;
                }
            }
        }
        model
            .clone()
            .on_hover_text("切换模型和推理力度")
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        if self.state.model_picker_open {
            let picker = self.draw_model_picker(
                ui.ctx(),
                tokens,
                egui::pos2(model_rect.right() - 300.0, model_rect.top() - 348.0),
            );
            let dismissed = ui.input(|input| {
                input.key_pressed(egui::Key::Escape)
                    || (input.pointer.any_click()
                        && !model_clicked
                        && !input
                            .pointer
                            .interact_pos()
                            .is_some_and(|pos| picker.contains(pos)))
            });
            if dismissed {
                self.state.model_picker_open = false;
            }
        }

        let slash_command_menu =
            self.state.composer.trim() == "/" && ui.memory(|memory| memory.has_focus(editor_id));
        if self.composer_menu_open || slash_command_menu {
            let anchor = egui::pos2(plus_rect.left(), plus_rect.top() - 224.0);
            let menu_area = self.draw_composer_menu(
                ui.ctx(),
                tokens,
                anchor,
                plus_clicked,
                slash_command_menu,
                editor_id,
            );
            let dismissed = ui.input(|input| {
                input.key_pressed(egui::Key::Escape)
                    || (input.pointer.any_click()
                        && !plus_clicked
                        && !input
                            .pointer
                            .interact_pos()
                            .is_some_and(|pos| menu_area.contains(pos)))
            });
            if dismissed {
                self.composer_menu_open = false;
                self.composer_submenu = None;
            }
        }
        if !self.state.status.is_empty() && self.state.connection == ConnectionState::Failed {
            // Errors stay available without forcing a long status string into
            // the carefully measured 30px action row.
            ui.interact(card, Id::new("composer-error"), Sense::hover())
                .on_hover_text(&self.state.status);
        }
        // The send button is the stop button while a turn runs, so the
        // shortcut is the only way to line the next prompt up. Losing it
        // silently would look like a broken Enter key; `send_current_message`
        // turns a busy send into a queue entry.
        if send_now && has_payload && self.state.selected_session.is_some() {
            self.send_current_message();
        }
        if stop_now {
            self.cancel_current_message();
        }
    }

    fn current_model_label(&self) -> &str {
        let pinned = self
            .state
            .selected()
            .and_then(|session| session.model_key.as_deref());
        if let Some(key) = pinned {
            return self
                .state
                .models
                .iter()
                .find(|model| model.key == key)
                .map(|model| model.model.as_str())
                .unwrap_or(key);
        }
        self.state
            .models
            .iter()
            .find(|model| model.is_default)
            .map(|model| model.model.as_str())
            .unwrap_or("默认模型")
    }

    fn draw_model_picker(
        &mut self,
        ctx: &Context,
        tokens: Tokens,
        anchor: egui::Pos2,
    ) -> egui::Rect {
        let selected_id = self.state.selected_session.clone();
        let selected = self.state.selected().cloned();
        let current_key = selected
            .as_ref()
            .and_then(|session| session.model_key.as_deref())
            .or_else(|| {
                self.state
                    .models
                    .iter()
                    .find(|model| model.is_default)
                    .map(|model| model.key.as_str())
            })
            .unwrap_or_default()
            .to_owned();
        let context_tokens = selected.as_ref().and_then(|session| session.context_tokens);
        let context_capacity = selected
            .as_ref()
            .and_then(|session| session.context_capacity_tokens)
            .or_else(|| {
                self.state
                    .models
                    .iter()
                    .find(|model| model.key == current_key)
                    .and_then(|model| model.max_context_tokens)
            });
        let reasoning_effort = selected
            .as_ref()
            .map(|session| session.reasoning_effort.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("default")
            .to_owned();
        let area = egui::Area::new(Id::new("composer-model-picker"))
            .order(Order::Foreground)
            .fixed_pos(anchor)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(tokens.surface)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(12.0)
                    .inner_margin(Margin::same(10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: tokens.shadow_panel,
                    })
                    .show(ui, |ui| {
                        ui.set_width(280.0);
                        ui.set_min_height(320.0);
                        let context_label = match (context_tokens, context_capacity) {
                            (Some(used), Some(capacity)) if capacity > 0 => {
                                format!(
                                    "{} / {} Token",
                                    format_tokens(used),
                                    format_tokens(capacity)
                                )
                            }
                            (Some(used), _) => format!("{} Token", format_tokens(used)),
                            _ => "—".to_owned(),
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("上下文 Token")
                                    .size(12.0)
                                    .color(tokens.text_secondary),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(context_label)
                                        .size(12.0)
                                        .color(tokens.text_muted),
                                );
                            });
                        });
                        ui.add_space(8.0);
                        if self.state.model_picker_loading {
                            ui.add_sized(
                                Vec2::new(280.0, 86.0),
                                egui::Label::new(
                                    RichText::new("正在读取模型…")
                                        .size(12.0)
                                        .color(tokens.text_muted),
                                )
                                .wrap(),
                            );
                        } else if self.state.models.is_empty() {
                            ui.vertical_centered(|ui| {
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new("还没有可用模型")
                                        .size(13.0)
                                        .color(tokens.text_secondary),
                                );
                                ui.add_space(6.0);
                                if ui.button("打开模型设置").clicked() {
                                    self.state.settings_category = "agent".to_owned();
                                    self.state.settings_open = true;
                                    self.state.model_picker_open = false;
                                }
                                ui.add_space(8.0);
                            });
                        } else {
                            let mut next_model = None;
                            ScrollArea::vertical()
                                .id_salt("model-picker-list")
                                .max_height(165.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for option in self.state.models.iter().take(128) {
                                        let is_current = option.key == current_key;
                                        let (row, response) = ui.allocate_exact_size(
                                            Vec2::new(280.0, 32.0),
                                            Sense::click(),
                                        );
                                        if response.hovered() || is_current {
                                            ui.painter().rect_filled(
                                                row,
                                                6.0,
                                                if is_current {
                                                    tokens.primary_tint
                                                } else {
                                                    tokens.hover
                                                },
                                            );
                                        }
                                        ui.painter().text(
                                            row.left_center() + Vec2::new(8.0, -6.0),
                                            egui::Align2::LEFT_CENTER,
                                            &option.model,
                                            egui::FontId::proportional(13.0),
                                            if is_current {
                                                tokens.primary
                                            } else {
                                                tokens.text
                                            },
                                        );
                                        ui.painter().text(
                                            row.left_center() + Vec2::new(8.0, 8.0),
                                            egui::Align2::LEFT_CENTER,
                                            &option.provider,
                                            egui::FontId::proportional(10.0),
                                            tokens.text_muted,
                                        );
                                        if is_current {
                                            crate::icons::paint(
                                                ui.painter(),
                                                egui::Rect::from_center_size(
                                                    row.right_center() - Vec2::new(12.0, 0.0),
                                                    Vec2::splat(12.0),
                                                ),
                                                crate::icons::Icon::Check,
                                                tokens.primary,
                                            );
                                        }
                                        if response.clicked()
                                            && !is_current
                                            && !self.state.model_picker_saving
                                        {
                                            next_model = Some(option.key.clone());
                                        }
                                    }
                                });
                            if let (Some(session_id), Some(model_key)) =
                                (selected_id.clone(), next_model)
                            {
                                let previous = self
                                    .state
                                    .sessions
                                    .iter()
                                    .find(|session| session.id == session_id)
                                    .and_then(|session| session.model_key.clone());
                                if let Some(session) = self
                                    .state
                                    .sessions
                                    .iter_mut()
                                    .find(|session| session.id == session_id)
                                {
                                    session.model_key = Some(model_key.clone());
                                }
                                self.pending_model_change = Some(PendingModelChange::Model {
                                    session_id: session_id.clone(),
                                    previous: previous.clone(),
                                });
                                self.state.model_picker_error.clear();
                                self.state.model_picker_saving = true;
                                if let Err(error) =
                                    self.backend.submit(BackendCommand::SetSessionModel {
                                        session_id: session_id.clone(),
                                        model_key,
                                    })
                                {
                                    self.state.model_picker_saving = false;
                                    if let Some(session) = self
                                        .state
                                        .sessions
                                        .iter_mut()
                                        .find(|session| session.id == session_id)
                                    {
                                        session.model_key = previous;
                                    }
                                    self.pending_model_change = None;
                                    self.state.model_picker_error = error;
                                }
                            }
                        }
                        if !self.state.model_picker_error.is_empty() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    self.state
                                        .model_picker_error
                                        .chars()
                                        .take(80)
                                        .collect::<String>(),
                                )
                                .size(11.0)
                                .color(tokens.danger),
                            );
                        }
                        ui.separator();
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("推理力度")
                                .size(12.0)
                                .color(tokens.text_secondary),
                        );
                        ui.add_space(5.0);
                        ui.horizontal_wrapped(|ui| {
                            for (value, label) in [
                                ("default", "默认"),
                                ("none", "不思考"),
                                ("minimal", "极少"),
                                ("low", "低"),
                                ("medium", "中"),
                                ("high", "高"),
                                ("xhigh", "极高"),
                            ] {
                                let active = reasoning_effort == value;
                                let button = egui::Button::new(
                                    RichText::new(label).size(11.0).color(if active {
                                        tokens.primary
                                    } else {
                                        tokens.text_secondary
                                    }),
                                )
                                .fill(if active {
                                    tokens.primary_tint
                                } else {
                                    tokens.surface
                                })
                                .stroke(Stroke::new(
                                    1.0,
                                    if active {
                                        tokens.primary
                                    } else {
                                        tokens.border
                                    },
                                ));
                                if ui
                                    .add_enabled(!self.state.model_picker_saving, button)
                                    .clicked()
                                    && !active
                                {
                                    if let Some(session_id) = selected_id.clone() {
                                        let previous = self
                                            .state
                                            .sessions
                                            .iter()
                                            .find(|session| session.id == session_id)
                                            .map(|session| session.reasoning_effort.clone())
                                            .unwrap_or_else(|| "default".to_owned());
                                        if let Some(session) = self
                                            .state
                                            .sessions
                                            .iter_mut()
                                            .find(|session| session.id == session_id)
                                        {
                                            session.reasoning_effort = value.to_owned();
                                        }
                                        self.pending_model_change =
                                            Some(PendingModelChange::Reasoning {
                                                session_id: session_id.clone(),
                                                previous: previous.clone(),
                                            });
                                        self.state.model_picker_error.clear();
                                        self.state.model_picker_saving = true;
                                        if let Err(error) = self.backend.submit(
                                            BackendCommand::SaveSessionReasoningEffort {
                                                session_id: session_id.clone(),
                                                value: value.to_owned(),
                                            },
                                        ) {
                                            self.state.model_picker_saving = false;
                                            if let Some(session) = self
                                                .state
                                                .sessions
                                                .iter_mut()
                                                .find(|session| session.id == session_id)
                                            {
                                                session.reasoning_effort = previous;
                                            }
                                            self.pending_model_change = None;
                                            self.state.model_picker_error = error;
                                        }
                                    }
                                }
                            }
                        });
                    });
            });
        area.response.rect
    }

    fn composer_menu_row(
        ui: &mut Ui,
        tokens: Tokens,
        icon: crate::icons::Icon,
        title: &str,
        selected: bool,
        has_flyout: bool,
    ) -> egui::Response {
        let (row, response) = ui.allocate_exact_size(Vec2::new(208.0, 34.0), Sense::click());
        if response.hovered() || selected {
            ui.painter().rect_filled(
                row,
                6.0,
                if selected {
                    tokens.primary_tint
                } else {
                    tokens.hover
                },
            );
        }
        let color = if selected {
            tokens.primary
        } else {
            tokens.text_secondary
        };
        crate::icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(row.left() + 16.0, row.center().y),
                Vec2::splat(15.0),
            ),
            icon,
            color,
        );
        ui.painter().text(
            egui::pos2(row.left() + 32.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(13.0),
            if selected {
                tokens.primary
            } else {
                tokens.text
            },
        );
        if has_flyout {
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    egui::pos2(row.right() - 12.0, row.center().y),
                    Vec2::splat(9.0),
                ),
                crate::icons::Icon::ChevronRight,
                tokens.text_muted,
            );
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn draw_composer_menu(
        &mut self,
        ctx: &Context,
        tokens: Tokens,
        anchor: egui::Pos2,
        _plus_clicked: bool,
        slash_command_menu: bool,
        editor_id: Id,
    ) -> egui::Rect {
        if slash_command_menu && !self.composer_menu_open {
            return self.draw_composer_flyout(
                ctx,
                tokens,
                anchor,
                ComposerSubmenu::Commands,
                editor_id,
            );
        }
        let main = egui::Area::new(Id::new("composer-menu"))
            .order(Order::Foreground)
            .fixed_pos(anchor)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(tokens.surface)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(12.0)
                    .inner_margin(Margin::same(6))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: tokens.shadow_popup,
                    })
                    .show(ui, |ui| {
                        ui.set_width(208.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let command = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Terminal,
                            "命令",
                            self.composer_submenu == Some(ComposerSubmenu::Commands),
                            true,
                        );
                        if command.clicked() {
                            self.composer_submenu = toggle_composer_submenu(
                                self.composer_submenu,
                                ComposerSubmenu::Commands,
                            );
                        }
                        let presets = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Bolt,
                            "预设问题",
                            self.composer_submenu == Some(ComposerSubmenu::Presets),
                            true,
                        );
                        if presets.clicked() {
                            self.composer_submenu = toggle_composer_submenu(
                                self.composer_submenu,
                                ComposerSubmenu::Presets,
                            );
                        }
                        let approval = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Shield,
                            "审批模式",
                            self.composer_submenu == Some(ComposerSubmenu::Approval),
                            true,
                        );
                        if approval.clicked() {
                            self.composer_submenu = toggle_composer_submenu(
                                self.composer_submenu,
                                ComposerSubmenu::Approval,
                            );
                        }
                        let voice = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Microphone,
                            "语音输入",
                            false,
                            false,
                        );
                        if voice.clicked() {
                            self.composer_menu_open = false;
                            self.composer_submenu = None;
                            self.state.status = "语音输入尚未接入 egui 原生适配器".to_owned();
                        }
                        let screenshot = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Camera,
                            "截图",
                            self.composer_submenu == Some(ComposerSubmenu::Screenshot),
                            true,
                        );
                        if screenshot.clicked() {
                            self.composer_submenu = toggle_composer_submenu(
                                self.composer_submenu,
                                ComposerSubmenu::Screenshot,
                            );
                        }
                        let terminal = Self::composer_menu_row(
                            ui,
                            tokens,
                            crate::icons::Icon::Terminal,
                            "终端模式",
                            self.terminal_mode,
                            false,
                        );
                        if self.terminal_mode {
                            crate::icons::paint(
                                ui.painter(),
                                egui::Rect::from_center_size(
                                    egui::pos2(
                                        terminal.rect.right() - 12.0,
                                        terminal.rect.center().y,
                                    ),
                                    Vec2::splat(11.0),
                                ),
                                crate::icons::Icon::Check,
                                tokens.primary,
                            );
                        }
                        if terminal.clicked() {
                            self.terminal_mode = !self.terminal_mode;
                            self.composer_menu_open = false;
                            self.composer_submenu = None;
                            self.state.status = if self.terminal_mode {
                                "终端模式入口已开启；独立终端窗口适配器正在接入".to_owned()
                            } else {
                                "已退出终端模式".to_owned()
                            };
                        }
                    });
            });
        let mut bounds = main.response.rect;
        if let Some(submenu) = self.composer_submenu {
            let row = match submenu {
                ComposerSubmenu::Commands => 0.0,
                ComposerSubmenu::Presets => 1.0,
                ComposerSubmenu::Approval => 2.0,
                ComposerSubmenu::Screenshot => 4.0,
            };
            let flyout_anchor = egui::pos2(
                main.response.rect.right() + 12.0,
                main.response.rect.top() + 6.0 + row * 36.0,
            );
            bounds = bounds.union(self.draw_composer_flyout(
                ctx,
                tokens,
                flyout_anchor,
                submenu,
                editor_id,
            ));
        }
        bounds
    }

    fn draw_composer_flyout(
        &mut self,
        ctx: &Context,
        tokens: Tokens,
        anchor: egui::Pos2,
        submenu: ComposerSubmenu,
        editor_id: Id,
    ) -> egui::Rect {
        let area = egui::Area::new(Id::new(("composer-menu-flyout", submenu as u8)))
            .order(Order::Foreground)
            .fixed_pos(anchor)
            .show(ctx, |ui| {
                Frame::NONE
                    .fill(tokens.surface)
                    .stroke(Stroke::new(1.0, tokens.border))
                    .corner_radius(10.0)
                    .inner_margin(Margin::same(8))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 3],
                        blur: 16,
                        spread: 0,
                        color: tokens.shadow_popup,
                    })
                    .show(ui, |ui| {
                        ui.set_width(252.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        match submenu {
                            ComposerSubmenu::Commands => {
                                for (value, label) in [
                                    ("/help", "帮助"),
                                    ("/new", "新建线程"),
                                    ("/stop", "停止当前任务"),
                                    ("/goal ", "设置目标"),
                                    ("/compact", "压缩上下文"),
                                ] {
                                    let response = Self::composer_flyout_row(ui, tokens, label, false);
                                    if response.clicked() {
                                        self.state.composer = value.to_owned();
                                        self.composer_menu_open = false;
                                        self.composer_submenu = None;
                                        ctx.memory_mut(|memory| memory.request_focus(editor_id));
                                    }
                                }
                            }
                            ComposerSubmenu::Presets => {
                                ui.label(RichText::new("预设问题").size(12.0).color(tokens.text));
                                for value in [
                                    "请先梳理目标、约束和可验证的交付结果。",
                                    "请根据现有资料给出可执行的下一步计划。",
                                ] {
                                    let response = Self::composer_flyout_row(ui, tokens, value, false);
                                    if response.clicked() {
                                        self.state.composer = value.to_owned();
                                        self.composer_menu_open = false;
                                        self.composer_submenu = None;
                                        ctx.memory_mut(|memory| memory.request_focus(editor_id));
                                    }
                                }
                            }
                            ComposerSubmenu::Approval => {
                                ui.label(RichText::new("审批模式").size(12.0).color(tokens.text));
                                for value in ["完全访问", "写入自动", "执行前确认"] {
                                    let response = Self::composer_flyout_row(
                                        ui,
                                        tokens,
                                        value,
                                        self.approval_mode == value,
                                    );
                                    if response.clicked() {
                                        self.approval_mode = value;
                                        self.composer_submenu = None;
                                        self.composer_menu_open = false;
                                    }
                                }
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new("审批规则由运行时执行；当前选择将在 native 适配器接入后保存。")
                                        .size(11.0)
                                        .color(tokens.text_muted),
                                );
                            }
                            ComposerSubmenu::Screenshot => {
                                ui.label(RichText::new("截图方式").size(12.0).color(tokens.text));
                                for value in [
                                    "全屏截图并隐藏窗口",
                                    "区域截图并隐藏窗口",
                                    "全屏截图并保留窗口",
                                    "区域截图并保留窗口",
                                ] {
                                    let response = Self::composer_flyout_row(ui, tokens, value, false);
                                    if response.clicked() {
                                        self.composer_menu_open = false;
                                        self.composer_submenu = None;
                                        self.state.status = "截图功能尚未接入 egui 原生适配器".to_owned();
                                    }
                                }
                            }
                        }
                    });
            });
        area.response.rect
    }

    fn composer_flyout_row(
        ui: &mut Ui,
        tokens: Tokens,
        label: &str,
        selected: bool,
    ) -> egui::Response {
        let (row, response) = ui.allocate_exact_size(Vec2::new(252.0, 28.0), Sense::click());
        if response.hovered() || selected {
            ui.painter().rect_filled(
                row,
                5.0,
                if selected {
                    tokens.primary_tint
                } else {
                    tokens.hover
                },
            );
        }
        ui.painter().text(
            row.left_center() + Vec2::new(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(12.0),
            if selected {
                tokens.primary
            } else {
                tokens.text
            },
        );
        if selected {
            crate::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(
                    row.right_center() - Vec2::new(10.0, 0.0),
                    Vec2::splat(12.0),
                ),
                crate::icons::Icon::Check,
                tokens.primary,
            );
        }
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    fn send_current_message(&mut self) {
        let Some(session_id) = self.state.selected_session.clone() else {
            self.state.status = "请先选择一个线程".to_owned();
            return;
        };
        let content = std::mem::take(&mut self.state.composer);
        if content.trim().is_empty() && self.state.attachments.is_empty() {
            return;
        }
        // Queueing lives here, not at the click site: the send button, the
        // Enter shortcut and any future caller must not race the backend while
        // a turn is running.
        if self.state.busy {
            self.enqueue_prompt(content);
            return;
        }
        let attachments = self.take_prompt_attachments();
        self.dispatch_prompt(session_id, content, attachments);
    }

    /// Move the pending attachment set into the command shape.
    ///
    /// Only paths travel: the worker reads the bytes, so a large document never
    /// stalls a frame. The set is cleared because the chips belonged to the
    /// message that is now on its way.
    fn take_prompt_attachments(&mut self) -> Vec<crate::backend::PromptAttachment> {
        let attachments = self
            .state
            .attachments
            .items()
            .iter()
            .map(|item| crate::backend::PromptAttachment {
                name: item.name.clone(),
                path: item.path.clone(),
            })
            .collect();
        self.state.attachments.clear();
        attachments
    }

    /// Put the draft behind the running turn instead of racing the backend.
    ///
    /// The queue holds text only, so pending attachments are folded in as path
    /// lines. Downstream that reads exactly like a user pasting a file list,
    /// which is what a queued run should see; a live send keeps the real
    /// attachment payload.
    fn enqueue_prompt(&mut self, content: String) {
        let mut queued = content.clone();
        for item in self.state.attachments.items() {
            if !queued.is_empty() {
                queued.push('\n');
            }
            queued.push_str("- 附件：");
            queued.push_str(&item.path);
        }
        match self.state.prompt_queue.push(queued) {
            Ok(()) => {
                self.state.attachments.clear();
                self.state.composer_notice =
                    format!("已加入待发送队列（{} 条）", self.state.prompt_queue.len());
            }
            Err(error) => {
                // Put the draft back so a full queue never eats a message.
                self.state.composer = content;
                self.state.composer_notice = error.message().to_owned();
            }
        }
    }

    /// One place that actually sends, shared by the shortcut and the queue.
    fn dispatch_prompt(
        &mut self,
        session_id: String,
        content: String,
        attachments: Vec<crate::backend::PromptAttachment>,
    ) {
        if !content.trim().is_empty() {
            self.state.prompt_history.push(&content);
        }
        self.state.composer_notice.clear();
        self.follow_tail = true;
        if let Err(error) = self.backend.submit(BackendCommand::SendMessage {
            session_id,
            content,
            attachments,
        }) {
            self.state.status = format!("无法发送消息：{error}");
        }
    }

    /// Send the oldest queued prompt once the running turn has finished.
    fn drain_prompt_queue(&mut self) {
        let Some(session_id) = self.state.selected_session.clone() else {
            return;
        };
        let Some(next) = self.state.prompt_queue.pop() else {
            return;
        };
        self.dispatch_prompt(session_id, next, Vec::new());
    }

    fn cancel_current_message(&mut self) {
        if let Some(session_id) = self.state.selected_session.clone() {
            self.submit(BackendCommand::Cancel { session_id });
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.perf.mark_frame();
        if !self.styled {
            theme::apply(ctx);
            self.styled = true;
        }
        self.pump(ctx);
        if self.last_selected != self.state.selected_session {
            if let Some(session_id) = self.state.selected_session.clone() {
                self.submit(BackendCommand::OpenSession(session_id));
                self.refresh_workspace_files();
            }
            self.last_selected = self.state.selected_session.clone();
        }
        self.draw_sidebar(ctx);
        self.draw_chat(ctx);
        crate::settings::draw(ctx, &mut self.state, theme::tokens());
        self.draw_system_prompt_preview(ctx);
        #[cfg(all(feature = "fixtures", not(target_arch = "wasm32")))]
        crate::capture::tick(ctx);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod sidebar_tests {
    use super::*;

    #[test]
    fn thread_title_validation_matches_native_facade_limits() {
        assert!(valid_thread_title("有效线程名"));
        assert!(!valid_thread_title("  "));
        assert!(!valid_thread_title("名称\n含换行"));
        assert!(!valid_thread_title(&"名".repeat(121)));
    }

    #[test]
    fn workspace_entry_path_rejects_escape_and_absolute_names() {
        assert!(valid_workspace_entry_path("notes/example.txt"));
        assert!(!valid_workspace_entry_path("../outside"));
        assert!(!valid_workspace_entry_path("C:\\outside"));
        assert!(!valid_workspace_entry_path("notes/../outside"));
    }

    #[test]
    fn sidebar_breakpoint_and_split_stay_within_hive_bounds() {
        assert!(!uses_compact_sidebar(1024.0));
        assert!(uses_compact_sidebar(1023.0));
        assert_eq!(clamp_sidebar_split(0.1), 0.30);
        assert_eq!(clamp_sidebar_split(0.60), 0.60);
        assert_eq!(clamp_sidebar_split(1.2), 0.80);
        assert_eq!(drawer_sidebar_width(900.0), 420.0);
        // Below the 170px minimum the 30px dismissal strip wins: the drawer
        // shrinks instead of covering the viewport edge.
        assert_eq!(drawer_sidebar_width(180.0), 150.0);
        assert_eq!(drawer_sidebar_width(200.0), 170.0);
    }

    #[test]
    fn splitter_maps_screen_pointer_to_a_bounded_workspace_share() {
        // 900px window: 867px track, so 858px is draggable above the splitter.
        let track = 900.0 - SIDEBAR_FOOTER_HEIGHT;
        assert_eq!(split_from_pointer(0.0, 0.0, track), 0.30);
        assert!((split_from_pointer(429.0, 0.0, track) - 0.50).abs() < 0.001);
        assert_eq!(split_from_pointer(10_000.0, 0.0, track), 0.80);
        // A sidebar shorter than the footer must not produce NaN or a
        // reversed track.
        let tiny = split_from_pointer(5.0, 0.0, 4.0);
        assert!(tiny.is_finite());
        assert_eq!(tiny, 0.80);
        // The top of the track is the workspace origin: offsetting it moves
        // the same pointer to a smaller share.
        assert!(split_from_pointer(429.0, 100.0, track) < 0.50);
    }

    fn sidebar_frame(ctx: &Context, app: &mut EguiApp, size: Vec2, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| app.draw_sidebar(ctx),
        );
    }

    fn click_at(pos: egui::Pos2) -> [Vec<egui::Event>; 3] {
        [
            vec![egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ]
    }

    /// egui resolves hover against the *previous* frame's widget rects, and a
    /// freshly shown drawer reports its full-viewport rect before its content
    /// settles. Two idle frames are needed before pointer input targets the
    /// layer the user actually sees.
    fn settle(ctx: &Context, app: &mut EguiApp, size: Vec2) {
        for _ in 0..2 {
            sidebar_frame(ctx, app, size, Vec::new());
        }
    }

    #[test]
    fn compact_drawer_opens_from_the_trigger_and_closes_on_scrim_or_escape() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let mut app = EguiApp::demo();
        let narrow = Vec2::new(980.0, 700.0);

        settle(&ctx, &mut app, narrow);
        assert!(
            !app.sidebar_open,
            "窄窗默认应保持抽屉关闭，只显示 30px 打开按钮"
        );

        // The trigger is an 8px inset 30px square in the top-left corner.
        for events in click_at(egui::pos2(23.0, 23.0)) {
            sidebar_frame(&ctx, &mut app, narrow, events);
        }
        assert!(app.sidebar_open, "点击 30px 打开按钮后抽屉应展开");

        // Escape closes the drawer before it reaches any other layer.
        sidebar_frame(
            &ctx,
            &mut app,
            narrow,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.sidebar_open, "Escape 应关闭窄窗抽屉");

        // Reopen, then dismiss by clicking the scrim outside the drawer.
        settle(&ctx, &mut app, narrow);
        for events in click_at(egui::pos2(23.0, 23.0)) {
            sidebar_frame(&ctx, &mut app, narrow, events);
        }
        assert!(app.sidebar_open);
        settle(&ctx, &mut app, narrow);
        let drawer_width = drawer_sidebar_width(narrow.x);
        for events in click_at(egui::pos2(drawer_width + 40.0, 350.0)) {
            sidebar_frame(&ctx, &mut app, narrow, events);
        }
        assert!(!app.sidebar_open, "点击抽屉外的遮罩应关闭抽屉");
    }

    #[test]
    fn desktop_sidebar_keeps_its_width_bounds_above_the_breakpoint() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let mut app = EguiApp::demo();
        sidebar_frame(&ctx, &mut app, Vec2::new(1440.0, 900.0), Vec::new());

        assert!(!app.sidebar_open, "宽视口不应打开抽屉");
        // The 30px drawer trigger only exists in the compact layout; at
        // 1440px the two-column shell must be used instead.
        assert!(
            ctx.memory(|memory| memory.area_rect(Id::new("messenger-sidebar-trigger")))
                .is_none(),
            "宽视口不应渲染窄窗抽屉按钮"
        );
    }

    #[test]
    fn failed_optimistic_rename_restores_the_previous_title() {
        let mut app = EguiApp::demo();
        let session_id = app.state.sessions[0].id.clone();
        let original_title = app.state.sessions[0].title.clone();
        app.state.sessions[0].title = "新名称".to_owned();
        app.pending_thread_renames.push(PendingThreadRename {
            session_id: session_id.clone(),
            original_title: original_title.clone(),
            requested_title: "新名称".to_owned(),
        });

        app.finish_thread_rename(session_id, "新名称".to_owned(), Err("写入失败".to_owned()));

        assert_eq!(app.state.sessions[0].title, original_title);
        assert_eq!(app.state.status, "无法重命名线程：写入失败");
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod composer_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    struct RecordingBackend(Rc<RefCell<Vec<BackendCommand>>>);

    impl BackendPort for RecordingBackend {
        fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
            self.0.borrow_mut().push(command);
            Ok(())
        }
        fn drain(&mut self, _budget: usize) -> Vec<crate::backend::BackendEvent> {
            Vec::new()
        }
    }

    fn frame(ctx: &Context, app: &mut EguiApp, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(960.0, 640.0),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| app.draw_chat(ctx),
        );
    }

    fn enter(modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: Some(egui::Key::Enter),
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn composer_preserves_newlines_and_ime_confirmation_before_sending() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let mut app = EguiApp::demo();
        let commands = Rc::new(RefCell::new(Vec::new()));
        app.backend = Box::new(RecordingBackend(commands.clone()));
        app.state.composer = "待发送内容".to_owned();
        frame(&ctx, &mut app, Vec::new());
        ctx.memory_mut(|memory| memory.request_focus(Id::new("chat-composer-input")));
        frame(&ctx, &mut app, Vec::new());

        frame(&ctx, &mut app, vec![enter(egui::Modifiers::SHIFT)]);
        assert!(commands.borrow().is_empty());
        assert!(app.state.composer.contains('\n'));

        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit("候选".to_owned())),
                enter(egui::Modifiers::NONE),
            ],
        );
        assert!(commands.borrow().is_empty());
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit("文本".to_owned())),
                enter(egui::Modifiers::NONE),
            ],
        );
        assert!(commands.borrow().is_empty());

        frame(&ctx, &mut app, vec![enter(egui::Modifiers::NONE)]);
        assert_eq!(commands.borrow().len(), 1);
        assert!(app.state.composer.is_empty());
        assert!(ctx.memory(|memory| memory.has_focus(Id::new("chat-composer-input"))));
    }

    #[test]
    fn composer_respects_ctrl_enter_setting() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let mut app = EguiApp::demo();
        let commands = Rc::new(RefCell::new(Vec::new()));
        app.backend = Box::new(RecordingBackend(commands.clone()));
        app.state.composer = "待发送内容".to_owned();
        app.state.settings_send_key = "Ctrl+Enter 发送".to_owned();
        frame(&ctx, &mut app, Vec::new());
        ctx.memory_mut(|memory| memory.request_focus(Id::new("chat-composer-input")));
        frame(&ctx, &mut app, Vec::new());

        frame(&ctx, &mut app, vec![enter(egui::Modifiers::NONE)]);
        assert!(commands.borrow().is_empty());
        assert!(app.state.composer.contains('\n'));
        frame(&ctx, &mut app, vec![enter(egui::Modifiers::CTRL)]);
        assert_eq!(commands.borrow().len(), 1);
        assert!(app.state.composer.is_empty());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod files_tests {
    use super::*;
    use crate::model::WorkspaceFileView;
    use std::{cell::RefCell, rc::Rc};

    struct RecordingBackend(Rc<RefCell<Vec<BackendCommand>>>);

    impl BackendPort for RecordingBackend {
        fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
            self.0.borrow_mut().push(command);
            Ok(())
        }
        fn drain(&mut self, _budget: usize) -> Vec<crate::backend::BackendEvent> {
            Vec::new()
        }
    }

    fn entry(name: &str, path: &str, is_directory: bool) -> WorkspaceFileView {
        WorkspaceFileView {
            name: name.to_owned(),
            path: path.to_owned(),
            is_directory,
            size: 0,
        }
    }

    fn app_with_files() -> (EguiApp, Rc<RefCell<Vec<BackendCommand>>>) {
        let mut app = EguiApp::demo();
        let commands = Rc::new(RefCell::new(Vec::new()));
        app.backend = Box::new(RecordingBackend(commands.clone()));
        app.state.selected_session = Some("fixture-thread".to_owned());
        // The demo fixture ships a pre-expanded tree; these tests own their own
        // listing so expansion and selection can be asserted exactly.
        app.state.workspace_tree = Default::default();
        app.state.workspace_selection = Default::default();
        app.state.selected_workspace_file = None;
        app.state.workspace_files = vec![
            entry("notes", "notes", true),
            entry("plan.md", "plan.md", false),
            entry("readme.md", "readme.md", false),
        ];
        (app, commands)
    }

    fn frame(ctx: &Context, app: &mut EguiApp, events: Vec<egui::Event>) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1440.0, 900.0),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| app.draw_sidebar(ctx),
        );
    }

    fn key(key: egui::Key) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    #[test]
    fn rename_accepts_only_a_single_path_segment() {
        assert!(valid_workspace_entry_name("plan.md"));
        assert!(valid_workspace_entry_name(" 笔记 "));
        assert!(!valid_workspace_entry_name(""));
        assert!(!valid_workspace_entry_name("notes/plan.md"));
        assert!(!valid_workspace_entry_name("notes\\plan.md"));
        assert!(!valid_workspace_entry_name(".."));
        assert!(!valid_workspace_entry_name("."));
        assert!(!valid_workspace_entry_name("bad\nname"));
        assert!(!valid_workspace_entry_name(&"名".repeat(121)));
    }

    #[test]
    fn keyboard_moves_the_directory_selection_and_opens_entries() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let (mut app, commands) = app_with_files();
        frame(&ctx, &mut app, Vec::new());
        // Clicking the empty part of the directory page arms keyboard
        // ownership without selecting a row.
        let (x, y) = (100.0, 800.0);
        for events in [
            vec![egui::Event::PointerMoved(egui::pos2(x, y))],
            vec![egui::Event::PointerButton {
                pos: egui::pos2(x, y),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: egui::pos2(x, y),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            frame(&ctx, &mut app, events);
        }
        assert_eq!(app.state.selected_workspace_file, None);

        frame(&ctx, &mut app, key(egui::Key::ArrowDown));
        assert_eq!(app.state.selected_workspace_file.as_deref(), Some("notes"));
        frame(&ctx, &mut app, key(egui::Key::ArrowDown));
        assert_eq!(
            app.state.selected_workspace_file.as_deref(),
            Some("plan.md")
        );
        frame(&ctx, &mut app, key(egui::Key::End));
        assert_eq!(
            app.state.selected_workspace_file.as_deref(),
            Some("readme.md")
        );
        frame(&ctx, &mut app, key(egui::Key::Home));
        assert_eq!(app.state.selected_workspace_file.as_deref(), Some("notes"));
        // Up on the first row stays clamped instead of wrapping to the end.
        frame(&ctx, &mut app, key(egui::Key::ArrowUp));
        assert_eq!(app.state.selected_workspace_file.as_deref(), Some("notes"));

        // Enter on a directory expands it and asks for its children rather
        // than opening the system handler or replacing the root listing.
        frame(&ctx, &mut app, key(egui::Key::Enter));
        {
            let submitted = commands.borrow();
            assert!(matches!(
                submitted.last(),
                Some(BackendCommand::ListWorkspaceChildren { path, .. }) if path == "notes"
            ));
        }
        assert!(app.state.workspace_tree.is_expanded("notes"));
        // Nothing about the root projection may change on expand.
        assert_eq!(app.state.workspace_files.len(), 3);
        assert_eq!(app.state.selected_workspace_file.as_deref(), Some("notes"));
    }

    #[test]
    fn delete_always_asks_before_submitting() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let (mut app, commands) = app_with_files();

        app.apply_files_action(FilesAction::Delete {
            path: "notes/archive".to_owned(),
            name: "archive".to_owned(),
            is_directory: true,
        });
        assert!(commands.borrow().is_empty(), "未确认前不得提交删除");
        assert!(app.delete_entry_confirm.is_some(), "应弹出删除确认");
        let Some(confirm) = app.delete_entry_confirm.as_ref() else {
            return;
        };
        assert_eq!(confirm.items.len(), 1);
        assert_eq!(confirm.items[0].name, "archive");
        assert!(confirm.items[0].is_directory);

        app.delete_entry_confirm = None;
        app.state.selected_workspace_file = Some("notes/archive".to_owned());
        app.apply_files_action(FilesAction::Delete {
            path: "notes/archive".to_owned(),
            name: "archive".to_owned(),
            is_directory: true,
        });
        // Confirming the same intent submits once and drops the stale row
        // selection so the panel cannot point at a deleted path.
        assert!(app.delete_entry_confirm.is_some(), "确认框仍在");
        let Some(dialog) = app.delete_entry_confirm.take() else {
            return;
        };
        app.state.selected_workspace_file = None;
        let Some(item) = dialog.items.first() else {
            return;
        };
        let _ = app.backend.submit(BackendCommand::DeleteWorkspaceEntry {
            session_id: "fixture-thread".to_owned(),
            path: item.path.clone(),
        });
        let submitted = commands.borrow();
        assert!(matches!(
            submitted.last(),
            Some(BackendCommand::DeleteWorkspaceEntry { path, .. }) if path == "notes/archive"
        ));
        assert_eq!(app.state.selected_workspace_file, None);
    }

    #[test]
    fn expanding_a_directory_requests_children_only_once() {
        let (mut app, commands) = app_with_files();
        app.apply_files_action(FilesAction::Toggle("notes".to_owned()));
        {
            let submitted = commands.borrow();
            assert!(matches!(
                submitted.last(),
                Some(BackendCommand::ListWorkspaceChildren { path, .. }) if path == "notes"
            ));
        }
        // The reply lands in the tree, not in the root projection.
        app.state
            .workspace_tree
            .set_children("notes", vec![entry("plan.md", "notes/plan.md", false)]);
        assert!(app.state.workspace_tree.is_expanded("notes"));

        let before = commands.borrow().len();
        app.apply_files_action(FilesAction::Toggle("notes".to_owned()));
        assert!(!app.state.workspace_tree.is_expanded("notes"));
        assert_eq!(commands.borrow().len(), before, "收起不应再发请求");

        // Re-expanding a cached directory must not hit the backend again.
        app.apply_files_action(FilesAction::Toggle("notes".to_owned()));
        assert!(app.state.workspace_tree.is_expanded("notes"));
        assert_eq!(commands.borrow().len(), before, "已缓存的目录不应重复请求");
    }

    #[test]
    fn refreshing_the_tree_collapses_it_before_relisting() {
        let (mut app, commands) = app_with_files();
        app.state
            .workspace_tree
            .set_children("notes", vec![entry("plan.md", "notes/plan.md", false)]);
        // `expand` reports whether a request is needed; cached children mean it
        // is not, but the directory must still be marked open.
        let _ = app.state.workspace_tree.expand("notes");
        assert!(app.state.workspace_tree.is_expanded("notes"));

        app.apply_files_action(FilesAction::Refresh);

        // Cached children point at paths that may be gone; the tree starts
        // collapsed so the next frame cannot show stale rows.
        assert!(!app.state.workspace_tree.is_expanded("notes"));
        let submitted = commands.borrow();
        assert!(matches!(
            submitted.last(),
            Some(BackendCommand::RefreshWorkspaceFiles { path, .. }) if path.is_empty()
        ));
    }

    #[test]
    fn multi_select_collects_a_batch_behind_one_confirmation() {
        let (mut app, commands) = app_with_files();
        app.apply_files_action(FilesAction::EnterMultiSelect);
        app.apply_files_action(FilesAction::ToggleSelection("plan.md".to_owned()));
        app.apply_files_action(FilesAction::ToggleSelection("readme.md".to_owned()));
        assert_eq!(app.state.workspace_selection.len(), 2);
        assert!(commands.borrow().is_empty());

        app.apply_files_action(FilesAction::DeleteSelected);
        {
            let confirm = app.delete_entry_confirm.as_ref();
            assert!(confirm.is_some(), "批量删除必须确认");
            let Some(confirm) = confirm else {
                return;
            };
            assert_eq!(confirm.items.len(), 2);
            assert!(confirm.items.iter().any(|item| item.path == "plan.md"));
        }
        assert!(commands.borrow().is_empty(), "确认前不得提交删除");

        // Leaving multi-select drops the selection so a stale path cannot be
        // acted on later.
        app.delete_entry_confirm = None;
        app.apply_files_action(FilesAction::ExitMultiSelect);
        assert!(app.state.workspace_selection.is_empty());
        assert!(!app.state.workspace_selection.is_active());
    }

    #[test]
    fn select_all_covers_exactly_the_visible_rows() {
        let (mut app, _commands) = app_with_files();
        app.state
            .workspace_tree
            .set_children("notes", vec![entry("plan.md", "notes/plan.md", false)]);
        app.apply_files_action(FilesAction::EnterMultiSelect);
        app.apply_files_action(FilesAction::SelectAllVisible);
        assert_eq!(
            app.state.workspace_selection.len(),
            3,
            "折叠状态下只应选中根级三行"
        );

        app.state.workspace_tree.expand("notes");
        app.apply_files_action(FilesAction::SelectAllVisible);
        assert_eq!(
            app.state.workspace_selection.len(),
            4,
            "展开后全选应包含子项"
        );
        // A folder with no children forms its own node only; expanding the
        // tree must not resurrect a batch from a previous frame.
        app.apply_files_action(FilesAction::SelectAllVisible);
        assert!(app.state.workspace_selection.is_empty(), "再次全选应清空");
    }

    #[test]
    fn rename_intent_prefills_the_current_name_and_parent() {
        let (mut app, _commands) = app_with_files();
        app.apply_files_action(FilesAction::Rename {
            path: "notes/plan.md".to_owned(),
            name: "plan.md".to_owned(),
        });
        assert!(app.workspace_prompt.is_some(), "应打开重命名输入框");
        let Some(prompt) = app.workspace_prompt.as_ref() else {
            return;
        };
        assert_eq!(prompt.draft, "plan.md");
        assert!(
            matches!(&prompt.kind, WorkspacePromptKind::Rename { path } if path == "notes/plan.md"),
            "重命名意图不得创建新条目"
        );
    }

    #[test]
    fn reference_appends_to_the_draft_without_touching_existing_text() {
        let (mut app, _commands) = app_with_files();
        app.state.composer = "看一下这个文件".to_owned();
        app.apply_files_action(FilesAction::Reference("notes/plan.md".to_owned()));
        assert_eq!(app.state.composer, "看一下这个文件\n@\"notes/plan.md\" ");

        app.apply_files_action(FilesAction::Reference("notes/plan.md".to_owned()));
        assert_eq!(
            app.state.composer,
            "看一下这个文件\n@\"notes/plan.md\" @\"notes/plan.md\" "
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod composer_attachment_tests {
    use super::*;
    use crate::composer_attachments::Attachment;
    use std::{cell::RefCell, rc::Rc};

    struct RecordingBackend(Rc<RefCell<Vec<BackendCommand>>>);

    impl BackendPort for RecordingBackend {
        fn submit(&mut self, command: BackendCommand) -> Result<(), String> {
            self.0.borrow_mut().push(command);
            Ok(())
        }
        fn drain(&mut self, _budget: usize) -> Vec<crate::backend::BackendEvent> {
            Vec::new()
        }
    }

    fn app() -> (EguiApp, Rc<RefCell<Vec<BackendCommand>>>) {
        let mut app = EguiApp::demo();
        let commands = Rc::new(RefCell::new(Vec::new()));
        app.backend = Box::new(RecordingBackend(commands.clone()));
        app.state.selected_session = Some("fixture-thread".to_owned());
        app.state.connection = ConnectionState::Ready;
        // The demo fixture ships attachments so the screenshot exercises the
        // chip strip; each test below starts from an empty draft instead.
        app.state.attachments.clear();
        app.state.composer.clear();
        (app, commands)
    }

    #[test]
    fn chip_rows_wrap_instead_of_overflowing() {
        // Zero chips reserve nothing; eight chips do not fit a 1000px card, and
        // the strip has to grow a second row rather than clip them.
        assert_eq!(composer_chip_rows(0, 1000.0), 0);
        assert_eq!(composer_chip_rows(4, 1000.0), 1);
        assert_eq!(composer_chip_rows(8, 1000.0), 2);
        assert_eq!(composer_strip_height(0, 0, 1000.0), 0.0);
        assert!(composer_strip_height(0, 1, 1000.0) > 0.0);
        assert!(composer_strip_height(8, 0, 1000.0) > composer_strip_height(4, 0, 1000.0));
        // A card narrower than one chip still gets a single column.
        assert_eq!(composer_chip_rows(2, 120.0), 2);
    }

    #[test]
    fn attachment_only_prompt_is_sent_with_its_payload() {
        let (mut app, commands) = app();
        assert!(app
            .state
            .attachments
            .add(Attachment::new("C:/tmp/notes.md", "notes.md", 2048))
            .is_ok());
        app.send_current_message();

        {
            let submitted = commands.borrow();
            assert_eq!(submitted.len(), 1);
            assert!(
                matches!(
                    &submitted[0],
                    BackendCommand::SendMessage { content, attachments, .. }
                        if content.is_empty() && attachments.len() == 1
                            && attachments[0].name == "notes.md"
                ),
                "空草稿加附件必须带上附件载荷发出"
            );
        }
        assert!(app.state.attachments.is_empty(), "发送后附件应清空");
    }

    #[test]
    fn busy_composer_queues_and_drains_after_the_turn() {
        let (mut app, commands) = app();
        app.state.busy = true;
        app.state.composer = "先跑一遍测试".to_owned();
        app.send_current_message();
        assert!(commands.borrow().is_empty(), "忙时不得直接发送");
        assert_eq!(app.state.prompt_queue.len(), 1);
        assert!(app.state.composer.is_empty(), "入队后草稿应清空");

        app.state.busy = false;
        app.drain_prompt_queue();
        let submitted = commands.borrow();
        assert!(
            matches!(
                submitted.last(),
                Some(BackendCommand::SendMessage { content, .. }) if content == "先跑一遍测试"
            ),
            "回合结束后应自动发出队首"
        );
    }

    #[test]
    fn a_full_queue_keeps_the_draft_instead_of_eating_it() {
        let (mut app, _commands) = app();
        for index in 0..crate::composer_attachments::MAX_QUEUED {
            assert!(app.state.prompt_queue.push(format!("排队 {index}")).is_ok());
        }
        app.state.busy = true;
        app.state.composer = "第五条".to_owned();
        app.send_current_message();

        assert_eq!(
            app.state.prompt_queue.len(),
            crate::composer_attachments::MAX_QUEUED
        );
        assert_eq!(app.state.composer, "第五条", "队列满时草稿必须留在输入框");
        assert!(app.state.composer_notice.contains("已满"));
    }

    #[test]
    fn sent_prompts_feed_the_up_arrow_history() {
        let (mut app, _commands) = app();
        app.state.composer = "第一条".to_owned();
        app.send_current_message();
        app.state.composer = "第二条".to_owned();
        app.send_current_message();

        let first = app.state.prompt_history.previous("");
        assert_eq!(first.as_deref(), Some("第二条"));
        let second = app.state.prompt_history.previous("");
        assert_eq!(second.as_deref(), Some("第一条"));
    }

    #[test]
    fn queued_prompt_keeps_its_attachment_paths_as_text() {
        let (mut app, _commands) = app();
        app.state.busy = true;
        app.state.composer = "看看这个".to_owned();
        assert!(app
            .state
            .attachments
            .add(Attachment::new("C:/tmp/plan.md", "plan.md", 64))
            .is_ok());
        app.send_current_message();

        let queued = app.state.prompt_queue.items();
        assert_eq!(queued.len(), 1);
        assert!(queued[0].contains("看看这个"));
        assert!(queued[0].contains("C:/tmp/plan.md"));
        assert!(app.state.attachments.is_empty());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod event_driven_tests {
    use super::*;
    use std::collections::VecDeque;

    struct QueuedBackend {
        events: VecDeque<crate::backend::BackendEvent>,
    }

    impl BackendPort for QueuedBackend {
        fn submit(&mut self, _command: BackendCommand) -> Result<(), String> {
            Ok(())
        }

        fn drain(&mut self, budget: usize) -> Vec<crate::backend::BackendEvent> {
            let take = budget.min(self.events.len());
            self.events.drain(..take).collect()
        }
    }

    #[test]
    fn pump_coalesces_deltas_before_the_reducer() {
        let ctx = Context::default();
        let mut app = EguiApp::demo();
        assert!(!app.state.turns.is_empty(), "demo transcript is empty");
        let session_id = app.state.selected_session.clone().unwrap_or_default();
        assert!(!session_id.is_empty(), "demo session is missing");
        let turn_id = app.state.turns[0].id.clone();
        let baseline = app.state.turns[0].assistant.text.clone();

        let mut events = VecDeque::new();
        for text in ["甲", "乙"] {
            events.push_back(crate::backend::BackendEvent::Delta {
                session_id: session_id.clone(),
                turn_id: turn_id.clone(),
                text: text.to_owned(),
            });
        }
        app.backend = Box::new(QueuedBackend { events });
        app.pump(&ctx);

        assert_eq!(app.state.turns[0].assistant.text, format!("{baseline}甲乙"));
        assert_eq!(app.perf_snapshot().events_applied, 1);
    }

    /// Run one pass through `pump` and report the soonest frame it scheduled.
    /// `Duration::MAX` means the frame loop may sleep until input or a wake.
    fn raw_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(960.0, 640.0),
            )),
            ..Default::default()
        }
    }

    /// File names that requested a repaint during one `pump` pass. A fresh
    /// context needs a second pass so egui rotates causes into `prev_causes`.
    fn repaint_sources(app: &mut EguiApp) -> Vec<&'static str> {
        let ctx = Context::default();
        let _ = ctx.run(raw_input(), |ctx| app.pump(ctx));
        let _ = ctx.run(raw_input(), |_| {});
        ctx.repaint_causes()
            .into_iter()
            .map(|cause| cause.file)
            .collect()
    }

    #[test]
    fn idle_frames_are_demand_driven() {
        let mut app = EguiApp::demo();
        app.state.busy = false;
        let sources = repaint_sources(&mut app);
        assert!(
            !sources.iter().any(|file| file.ends_with("app.rs")),
            "an idle pump must not schedule a repaint: {sources:?}"
        );
    }

    #[test]
    fn only_starting_or_running_states_keep_a_clock() {
        assert_eq!(
            scheduled_frame_delay(ConnectionState::Ready, false, false),
            None
        );
        assert_eq!(
            scheduled_frame_delay(ConnectionState::Ready, true, false),
            Some(IDLE_CLOCK_REPAINT)
        );
        assert_eq!(
            scheduled_frame_delay(ConnectionState::Starting, false, false),
            Some(IDLE_CLOCK_REPAINT)
        );
        assert_eq!(
            scheduled_frame_delay(ConnectionState::Ready, false, true),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_saturated_drain_schedules_the_remainder() {
        let mut events = VecDeque::new();
        for index in 0..(EVENT_DRAIN_BUDGET + 7) {
            events.push_back(crate::backend::BackendEvent::Queued {
                session_id: format!("thread-{index}"),
            });
        }
        let mut app = EguiApp::demo();
        app.backend = Box::new(QueuedBackend { events });

        let ctx = Context::default();
        app.pump(&ctx);
        assert_eq!(
            app.perf_snapshot().events_applied as usize,
            EVENT_DRAIN_BUDGET
        );

        app.pump(&ctx);
        assert_eq!(
            app.perf_snapshot().events_applied as usize,
            EVENT_DRAIN_BUDGET + 7
        );
    }
}
