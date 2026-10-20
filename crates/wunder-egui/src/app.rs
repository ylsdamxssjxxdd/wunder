use crate::backend::{apply_event, BackendCommand, BackendPort};
use crate::model::{AppState, ConnectionState, SessionView, TurnView};
use crate::theme::{self, Tokens};
use egui::{
    Align, CentralPanel, Color32, Context, Frame, Id, Layout, Margin, Order, RichText, ScrollArea,
    Sense, SidePanel, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};
use std::collections::{BTreeSet, VecDeque};
use std::time::Duration;

const SIDEBAR_WIDTH: f32 = 240.0;
const SIDEBAR_MIN_WIDTH: f32 = 170.0;
const SIDEBAR_MAX_WIDTH: f32 = 420.0;
const COMPACT_SIDEBAR_BREAKPOINT: f32 = 1024.0;
const DEFAULT_SIDEBAR_SPLIT: f32 = 0.60;
const CHAT_COLUMN_WIDTH: f32 = 1000.0;
/// Height of the draggable band between the workspace and directory panes.
const SIDEBAR_SPLITTER_HEIGHT: f32 = 9.0;
/// Height of the account row pinned to the bottom of the sidebar.
const SIDEBAR_FOOTER_HEIGHT: f32 = 33.0;

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

enum ThreadRowAction {
    Select(String),
    Rename { id: String, title: String },
    Trajectory { id: String, title: String },
    Archive(String),
    BeginMultiSelect,
    ToggleSelection(String),
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

#[derive(Clone, Copy)]
enum WorkspacePromptKind {
    Directory,
    File,
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
    pub state: AppState,
    styled: bool,
    last_selected: Option<String>,
    follow_tail: bool,
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
    thread_selection_mode: bool,
    selected_thread_ids: BTreeSet<String>,
    pending_thread_archives: VecDeque<String>,
    archive_in_flight: Option<String>,
    archive_batch_total: usize,
    archive_batch_succeeded: usize,
    archive_batch_failed: usize,
    workspace_prompt: Option<WorkspacePromptDialog>,
    workspace_prompt_needs_focus: bool,
    pending_model_change: Option<PendingModelChange>,
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
        let mut app = Self {
            backend: Box::new(crate::backend::NativeBackend::start()),
            state: AppState::default(),
            styled: false,
            last_selected: None,
            follow_tail: true,
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
            thread_selection_mode: false,
            selected_thread_ids: BTreeSet::new(),
            pending_thread_archives: VecDeque::new(),
            archive_in_flight: None,
            archive_batch_total: 0,
            archive_batch_succeeded: 0,
            archive_batch_failed: 0,
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            pending_model_change: None,
        };
        let _ = app.backend.submit(BackendCommand::RefreshSessions);
        ctx.request_repaint_after(Duration::from_millis(120));
        app
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn demo() -> Self {
        let state = crate::fixtures::demo_state();
        let last_selected = state.selected_session.clone();
        Self {
            backend: Box::new(crate::backend::DemoBackend::default()),
            follow_tail: state.should_follow_tail,
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
            thread_selection_mode: false,
            selected_thread_ids: BTreeSet::new(),
            pending_thread_archives: VecDeque::new(),
            archive_in_flight: None,
            archive_batch_total: 0,
            archive_batch_succeeded: 0,
            archive_batch_failed: 0,
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            pending_model_change: None,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn web(ctx: Context) -> Self {
        let app = Self {
            backend: Box::new(crate::backend::RemoteBackend::start()),
            state: AppState::default(),
            styled: false,
            last_selected: None,
            follow_tail: true,
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
            thread_selection_mode: false,
            selected_thread_ids: BTreeSet::new(),
            pending_thread_archives: VecDeque::new(),
            archive_in_flight: None,
            archive_batch_total: 0,
            archive_batch_succeeded: 0,
            archive_batch_failed: 0,
            workspace_prompt: None,
            workspace_prompt_needs_focus: false,
            pending_model_change: None,
        };
        ctx.request_repaint_after(Duration::from_millis(120));
        app
    }

    fn pump(&mut self, ctx: &Context) {
        let event_count = self.backend.poll(64).into_iter().fold(0, |count, event| {
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
                    }
                    Err(error) => self.state.status = format!("无法新建线程：{error}"),
                },
                crate::backend::BackendEvent::SessionArchived { session_id, result } => {
                    self.finish_thread_archive(session_id, result);
                }
                crate::backend::BackendEvent::WorkspaceFiles {
                    session_id,
                    path,
                    files,
                } if self.state.selected_session.as_deref() == Some(session_id.as_str()) => {
                    self.state.workspace_file_loading = false;
                    self.state.workspace_file_path = path;
                    match files {
                        Ok(files) => {
                            self.state.replace_workspace_files(files);
                            self.state.workspace_file_error.clear();
                        }
                        Err(error) => self.state.workspace_file_error = error,
                    }
                }
                crate::backend::BackendEvent::WorkspaceAction { label, result } => match result {
                    Ok(()) => {
                        self.state.status = label;
                        self.refresh_workspace_files();
                    }
                    Err(error) => self.state.status = error,
                },
                event => apply_event(&mut self.state, event),
            }
            count + 1
        });
        if self.state.connection == ConnectionState::Starting
            || self.state.busy
            || self.state.prompt_preview.loading
            || self.state.model_picker_loading
            || self.state.model_picker_saving
            || event_count > 0
        {
            ctx.request_repaint_after(if self.state.busy {
                Duration::from_millis(16)
            } else {
                Duration::from_millis(120)
            });
        }
    }

    fn submit(&mut self, command: BackendCommand) {
        if let Err(error) = self.backend.submit(command) {
            self.state.status = error;
        }
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

    fn draw_workspace_region(&mut self, ui: &mut Ui, tokens: Tokens) {
        ui.add_space(4.0);
        let (heading, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
        ui.painter().text(
            heading.left_center() + Vec2::new(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "工作区",
            egui::FontId::proportional(12.0),
            tokens.text_muted,
        );
        let (row, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
        let row = row.shrink2(Vec2::new(6.0, 0.0));
        ui.painter().rect_filled(row, 8.0, tokens.primary_tint);
        crate::icons::paint(
            ui.painter(),
            egui::Rect::from_center_size(
                row.left_center() + Vec2::new(14.0, 0.0),
                Vec2::splat(14.0),
            ),
            crate::icons::Icon::Folder,
            tokens.primary,
        );
        ui.painter().text(
            row.left_center() + Vec2::new(28.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "本地工作区",
            egui::FontId::proportional(14.0),
            tokens.text,
        );
        let plus = egui::Rect::from_center_size(
            egui::pos2(row.right() - 17.0, row.center().y),
            Vec2::splat(22.0),
        );
        let response = ui
            .interact(plus, Id::new("new-thread"), Sense::click())
            .on_hover_text("新建线程");
        if response.hovered() {
            ui.painter().rect_filled(plus, 6.0, tokens.surface);
        }
        crate::icons::paint(
            ui.painter(),
            plus.shrink(5.0),
            crate::icons::Icon::Plus,
            tokens.text_muted,
        );
        ui.painter().text(
            egui::pos2(plus.left() - 6.0, row.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{} 个线程", self.state.sessions.len()),
            egui::FontId::proportional(11.0),
            tokens.text_muted,
        );
        if response.clicked() {
            self.submit(BackendCommand::CreateSession);
        }
        let mut action = None;
        if self.state.sessions.is_empty() {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("暂无线程")
                        .size(12.0)
                        .color(tokens.text_muted),
                );
            });
        } else {
            let selection_bar_height = if self.thread_selection_mode {
                38.0
            } else {
                0.0
            };
            ScrollArea::vertical()
                .id_salt("thread-tree")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - selection_bar_height).max(0.0))
                .show_rows(ui, 28.0, self.state.sessions.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for index in range {
                        if let Some(next_action) = Self::draw_thread_row(
                            ui,
                            &self.state.sessions[index],
                            self.state.selected_session.as_deref(),
                            self.thread_selection_mode,
                            self.selected_thread_ids
                                .contains(&self.state.sessions[index].id),
                            tokens,
                        ) {
                            action = Some(next_action);
                        }
                    }
                });
            if self.thread_selection_mode {
                self.draw_thread_selection_bar(ui, tokens);
            }
        }
        if let Some(action) = action {
            self.handle_thread_row_action(action);
        }
    }

    fn draw_files_region(&mut self, ui: &mut Ui, tokens: Tokens) {
        let full = ui.max_rect();
        // Register the directory background before the file rows. egui gives
        // later widgets precedence, so a row keeps its click/selection input
        // instead of the panel's context-menu hitbox consuming it.
        let directory_response = ui
            .interact(full, Id::new("working-directory-menu"), Sense::click())
            .on_hover_text("右键打开工作目录菜单")
            .on_hover_cursor(egui::CursorIcon::ContextMenu);
        let head = egui::Rect::from_min_size(full.min, Vec2::new(full.width(), 24.0));
        ui.painter().text(
            head.left_center() + Vec2::new(12.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "工作目录",
            egui::FontId::proportional(12.0),
            tokens.text_muted,
        );
        let body = egui::Rect::from_min_max(head.left_bottom(), full.max);
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
        } else {
            ui.scope_builder(
                UiBuilder::new().max_rect(body.shrink2(Vec2::new(6.0, 3.0))),
                |ui| {
                    ScrollArea::vertical()
                        .id_salt("workspace-file-list")
                        .auto_shrink([false, false])
                        .show_rows(ui, 26.0, self.state.workspace_files.len(), |ui, range| {
                            for index in range {
                                let file = self.state.workspace_files[index].clone();
                                let (row, response) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 26.0),
                                    Sense::click(),
                                );
                                let selected = self.state.selected_workspace_file.as_deref()
                                    == Some(file.path.as_str());
                                if selected || response.hovered() {
                                    ui.painter().rect_filled(
                                        row,
                                        5.0,
                                        if selected {
                                            tokens.selected
                                        } else {
                                            tokens.hover
                                        },
                                    );
                                }
                                crate::icons::paint(
                                    ui.painter(),
                                    egui::Rect::from_center_size(
                                        row.left_center() + Vec2::new(10.0, 0.0),
                                        Vec2::splat(14.0),
                                    ),
                                    if file.is_directory {
                                        crate::icons::Icon::Folder
                                    } else {
                                        crate::icons::Icon::File
                                    },
                                    if file.is_directory {
                                        tokens.warning
                                    } else {
                                        tokens.text_muted
                                    },
                                );
                                ui.painter().text(
                                    row.left_center() + Vec2::new(21.0, 0.0),
                                    egui::Align2::LEFT_CENTER,
                                    &file.name,
                                    egui::FontId::proportional(12.0),
                                    tokens.text_secondary,
                                );
                                if !file.is_directory && file.size > 0 {
                                    ui.painter().text(
                                        row.right_center() - Vec2::new(3.0, 0.0),
                                        egui::Align2::RIGHT_CENTER,
                                        format!("{} B", file.size),
                                        egui::FontId::proportional(10.0),
                                        tokens.text_muted,
                                    );
                                }
                                if response.double_clicked() {
                                    self.open_workspace_file(file.path.clone());
                                } else if response.clicked() {
                                    self.state.select_workspace_file(file.path.clone());
                                }
                                response.context_menu(|menu| {
                                    if menu.button("刷新").clicked() {
                                        self.refresh_workspace_files();
                                        menu.close();
                                    }
                                    if menu
                                        .button(if file.is_directory {
                                            "打开目录"
                                        } else {
                                            "打开"
                                        })
                                        .clicked()
                                    {
                                        self.open_workspace_file(file.path.clone());
                                        menu.close();
                                    }
                                    menu.separator();
                                    if menu.button("重命名").clicked() {
                                        self.state.status = "工作目录条目重命名暂未接入".to_owned();
                                        menu.close();
                                    }
                                });
                            }
                        });
                },
            );
        }
        let mut directory_action = None;
        directory_response.context_menu(|ui| {
            if ui.button("新建目录").clicked() {
                directory_action = Some(WorkspacePromptKind::Directory);
                ui.close();
            }
            if ui.button("新建文件").clicked() {
                directory_action = Some(WorkspacePromptKind::File);
                ui.close();
            }
            ui.separator();
            if ui.button("打开本地工作区").clicked() {
                if let Some(session_id) = self.state.selected_session.clone() {
                    self.submit(BackendCommand::OpenLocalWorkspace(session_id));
                } else {
                    self.state.status = "请先选择一个线程".to_owned();
                }
                ui.close();
            }
            ui.separator();
            if ui.button("刷新").clicked() {
                self.refresh_workspace_files();
                ui.close();
            }
        });
        if let Some(kind) = directory_action {
            self.workspace_prompt = Some(WorkspacePromptDialog {
                kind,
                draft: String::new(),
            });
            self.workspace_prompt_needs_focus = true;
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
                if ui.button("重命名").clicked() {
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
                    ui.painter().rect_filled(button, 4.0, tokens.selected);
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
            ThreadRowAction::BeginMultiSelect => {
                self.thread_selection_mode = true;
                self.selected_thread_ids.clear();
            }
            ThreadRowAction::ToggleSelection(id) => {
                let archivable = self
                    .state
                    .sessions
                    .iter()
                    .any(|session| session.id == id && !session.locked);
                if archivable && !self.selected_thread_ids.remove(&id) {
                    self.selected_thread_ids.insert(id);
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
        ui.scope_builder(
            UiBuilder::new().max_rect(rect.shrink2(Vec2::new(6.0, 5.0))),
            |ui| {
                ui.horizontal(|ui| {
                    let archivable: Vec<&str> = self
                        .state
                        .sessions
                        .iter()
                        .filter(|session| !session.locked)
                        .map(|session| session.id.as_str())
                        .collect();
                    let all_selected = !archivable.is_empty()
                        && archivable
                            .iter()
                            .all(|id| self.selected_thread_ids.contains(*id));
                    if ui
                        .small_button(if all_selected {
                            "取消全选"
                        } else {
                            "全选"
                        })
                        .clicked()
                    {
                        if all_selected {
                            self.selected_thread_ids.clear();
                        } else {
                            self.selected_thread_ids =
                                archivable.into_iter().map(str::to_owned).collect();
                        }
                    }
                    if ui.small_button("退出").clicked() {
                        self.thread_selection_mode = false;
                        self.selected_thread_ids.clear();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let can_archive = !self.selected_thread_ids.is_empty()
                            && self.archive_in_flight.is_none()
                            && self.pending_thread_archives.is_empty();
                        if ui
                            .add_enabled(can_archive, egui::Button::new("归档"))
                            .clicked()
                        {
                            let ids = self.selected_thread_ids.iter().cloned().collect();
                            self.archive_threads(ids);
                        }
                        ui.label(
                            RichText::new(format!("已选 {} 项", self.selected_thread_ids.len()))
                                .size(11.0)
                                .color(tokens.text_muted),
                        );
                    });
                });
            },
        );
    }

    /// Archive in a bounded, sequential flow. The native facade cancels an
    /// active thread before archiving it, so launching every checked row at
    /// once would waste runtime capacity and make result ordering unstable.
    fn archive_threads(&mut self, ids: Vec<String>) {
        if self.archive_in_flight.is_some() || !self.pending_thread_archives.is_empty() {
            self.state.status = "归档操作正在进行".to_owned();
            return;
        }
        let ids: Vec<String> = ids
            .into_iter()
            .filter(|id| {
                self.state
                    .sessions
                    .iter()
                    .any(|session| session.id == *id && !session.locked)
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if ids.is_empty() {
            self.state.status = "没有可归档的线程".to_owned();
            return;
        }
        self.archive_batch_total = ids.len();
        self.archive_batch_succeeded = 0;
        self.archive_batch_failed = 0;
        self.pending_thread_archives = ids.into();
        self.state.status = "正在归档线程".to_owned();
        self.submit_next_thread_archive();
    }

    fn submit_next_thread_archive(&mut self) {
        if self.archive_in_flight.is_some() {
            return;
        }
        let Some(session_id) = self.pending_thread_archives.pop_front() else {
            self.finish_thread_archive_batch();
            return;
        };
        self.archive_in_flight = Some(session_id.clone());
        if let Err(error) = self
            .backend
            .submit(BackendCommand::ArchiveSession(session_id.clone()))
        {
            self.finish_thread_archive(session_id, Err(error));
        }
    }

    fn finish_thread_archive(&mut self, session_id: String, result: Result<(), String>) {
        if self.archive_in_flight.as_deref() != Some(session_id.as_str()) {
            return;
        }
        self.archive_in_flight = None;
        match result {
            Ok(()) => {
                self.archive_batch_succeeded += 1;
                self.selected_thread_ids.remove(&session_id);
                self.state
                    .sessions
                    .retain(|session| session.id != session_id);
                if self.state.selected_session.as_deref() == Some(session_id.as_str()) {
                    self.state.selected_session = self.state.sessions.first().map(|s| s.id.clone());
                    self.last_selected = None;
                    self.state.selected_workspace_file = None;
                }
            }
            Err(error) => {
                self.archive_batch_failed += 1;
                self.state.status = format!("无法归档线程：{error}");
            }
        }
        self.submit_next_thread_archive();
    }

    fn finish_thread_archive_batch(&mut self) {
        if self.archive_batch_total == 0 {
            return;
        }
        let total = self.archive_batch_total;
        let succeeded = self.archive_batch_succeeded;
        let failed = self.archive_batch_failed;
        self.archive_batch_total = 0;
        self.archive_batch_succeeded = 0;
        self.archive_batch_failed = 0;
        if total == 1 && succeeded == 1 {
            self.state.status = "线程已归档".to_owned();
        } else if failed == 0 {
            self.state.status = format!("已归档 {succeeded} 个线程");
        } else {
            self.state.status = format!("已归档 {succeeded} 个线程，{failed} 个失败");
        }
        self.thread_selection_mode = false;
        self.selected_thread_ids.clear();
    }

    fn refresh_workspace_files(&mut self) {
        let Some(session_id) = self.state.selected_session.clone() else {
            return;
        };
        self.state.begin_workspace_refresh();
        self.submit(BackendCommand::RefreshWorkspaceFiles {
            session_id,
            path: String::new(),
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
        let (title, hint) = match dialog.kind {
            WorkspacePromptKind::Directory => ("新建目录", "目录名，例如 notes/archive"),
            WorkspacePromptKind::File => ("新建文件", "文件名，例如 notes/example.txt"),
        };
        egui::Window::new(title)
            .id(Id::new("workspace-create-dialog"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(340.0);
                ui.label("相对当前工作目录输入路径：");
                ui.add_space(6.0);
                let input = ui.add(
                    TextEdit::singleline(&mut dialog.draft)
                        .id(Id::new("workspace-create-input"))
                        .desired_width(f32::INFINITY)
                        .hint_text(hint),
                );
                if self.workspace_prompt_needs_focus {
                    input.request_focus();
                    self.workspace_prompt_needs_focus = false;
                }
                let valid = valid_workspace_entry_path(&dialog.draft);
                if !dialog.draft.trim().is_empty() && !valid {
                    ui.label(
                        RichText::new("请输入相对路径，不能包含 .. 或控制字符")
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
            if let Some(session_id) = self.state.selected_session.clone() {
                let path = dialog.draft.trim().to_owned();
                match dialog.kind {
                    WorkspacePromptKind::Directory => {
                        self.submit(BackendCommand::CreateWorkspaceDirectory { session_id, path })
                    }
                    WorkspacePromptKind::File => {
                        self.submit(BackendCommand::CreateWorkspaceFile { session_id, path })
                    }
                }
            } else {
                self.state.status = "请先选择一个线程".to_owned();
            }
        } else if open {
            self.workspace_prompt = Some(dialog);
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
                let text_width = (full.width() - 32.0 - ruler_width)
                    .min(CHAT_COLUMN_WIDTH)
                    .max(80.0)
                    - 30.0;
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
                let composer_height = input_height + 84.0;
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
                                    // Clone one bounded projection at a time so a
                                    // process disclosure can mark its load state
                                    // and enqueue a page request while the list is
                                    // being painted.  Process details are capped
                                    // by the backend, so this does not copy an
                                    // unbounded transcript.
                                    for index in 0..self.state.turns.len() {
                                        let turn = self.state.turns[index].clone();
                                        if !turn.user.text.is_empty() {
                                            number += 1;
                                        }
                                        open_prompt_preview |=
                                            self.draw_turn(ui, &turn, number, tokens);
                                    }
                                }
                            },
                        );
                        ui.add_space((full.height() * 0.13).clamp(84.0, 132.0));
                    })
            })
            .inner;
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

    fn draw_turn_ruler(&self, ui: &mut Ui, viewport: egui::Rect, tokens: Tokens) {
        let count = self
            .state
            .turns
            .iter()
            .filter(|turn| !turn.user.text.is_empty())
            .count();
        // 与蜂巢 MessengerTurnRuler 的行为一致：单轮时不显示可点刻度，
        // 避免给空白聊天区增加无意义的视觉噪声。
        if count < 2 || viewport.height() < 48.0 {
            return;
        }

        let ruler_width = 18.0;
        let right = viewport.right() - 2.0;
        let top = viewport.top() + 10.0;
        let height = (viewport.height() - 20.0).max(1.0);
        let slot = height / count as f32;
        let mut hovered = None;
        for (index, (turn_index, turn)) in self
            .state
            .turns
            .iter()
            .enumerate()
            .filter(|(_, turn)| !turn.user.text.is_empty())
            .enumerate()
        {
            let center_y = top + slot * (index as f32 + 0.5);
            let hit = egui::Rect::from_center_size(
                egui::pos2(right - ruler_width * 0.5, center_y),
                egui::vec2(ruler_width, slot.max(12.0)),
            );
            let response = ui.interact(hit, Id::new(("turn-ruler", index)), Sense::hover());
            let is_hovered = response.hovered();
            if is_hovered {
                hovered = Some((turn_index, index + 1, response));
            }

            // 用用户输入长度模拟蜂巢的波感宽度：短消息为细刻度，
            // 长消息略宽，仍保持 2px 高和右侧 3px 内缩。
            let user_len = turn.user.text.chars().count().min(160) as f32;
            let width = (4.0 + user_len.sqrt() * 0.9).clamp(4.0, 11.0);
            let tick = egui::Rect::from_center_size(
                egui::pos2(right - 3.0 - width * 0.5, center_y),
                egui::vec2(width, if is_hovered { 3.0 } else { 2.0 }),
            );
            ui.painter().rect_filled(
                tick,
                1.0,
                if is_hovered {
                    tokens.primary
                } else {
                    tokens.border_strong
                },
            );
        }

        if let Some((index, number, response)) = hovered {
            let turn = &self.state.turns[index];
            let title = if turn.user.text.trim().is_empty() {
                format!("第 {} 轮", number)
            } else {
                format!("第 {} 轮  {}", number, turn.user.text.trim())
            };
            let answer = turn.assistant.text.trim();
            let preview = if answer.is_empty() {
                "正在生成…".to_owned()
            } else {
                answer.chars().take(120).collect::<String>()
            };
            // Tooltip 使用 egui 原生浮层，避免在消息滚动区内创建额外布局，
            // 视觉上对应蜂巢刻度左侧 264px 的悬停预览卡。
            response.on_hover_ui(|ui| {
                ui.set_max_width(264.0);
                ui.label(RichText::new(title).size(13.0).strong().color(tokens.text));
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

    fn draw_turn(&mut self, ui: &mut Ui, turn: &TurnView, number: usize, tokens: Tokens) -> bool {
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
        self.draw_workflow(ui, turn, tokens);
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
                    crate::markdown::show(ui, &turn.assistant.text, tokens.text);
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
    fn request_process_page(&mut self, turn_id: &str) {
        self.request_process_page_at(turn_id, None, None);
    }

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

    fn draw_workflow(&mut self, ui: &mut Ui, turn: &TurnView, tokens: Tokens) {
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
                    self.request_process_page(&turn.id);
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
                self.request_process_page(&turn.id);
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
                                self.request_process_page(&turn.id);
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
                            self.request_process_page_at(&turn.id, turn.process_first, None);
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
                            self.request_process_page_at(&turn.id, None, turn.process_last);
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
        let label = format!("#  第 {number} 个用户轮次");
        let galley =
            ui.painter()
                .layout_no_wrap(label, egui::FontId::proportional(11.0), tokens.text_muted);
        let chip = egui::Rect::from_min_size(
            rect.min + Vec2::new(24.0, 1.0),
            Vec2::new(galley.size().x + 16.0, 20.0),
        );
        ui.painter()
            .rect_filled(chip, 9.0, tokens.success_tint);
        ui.painter().rect_stroke(
            chip,
            9.0,
            Stroke::new(1.0, tokens.success_border),
            egui::StrokeKind::Inside,
        );
        ui.painter().galley(
            egui::pos2(chip.left() + 8.0, chip.center().y - galley.size().y / 2.0),
            galley,
            tokens.text_muted,
        );
        let status = match turn.assistant.state.as_str() {
            "streaming" | "running" => Some(("生成中", tokens.text_muted)),
            "error" | "failed" => Some(("生成失败", tokens.danger)),
            _ => None,
        };
        if let Some((label, color)) = status {
            ui.painter().text(
                egui::pos2(chip.right() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(11.0),
                color,
            );
        }
        let copy_rect = egui::Rect::from_min_size(
            egui::pos2(rect.right() - 46.0, rect.top()),
            Vec2::splat(22.0),
        );
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

    fn draw_composer(&mut self, ui: &mut Ui, tokens: Tokens) {
        use crate::icons::{self, Icon};
        let full = ui.max_rect();
        let ruler_width = if self.state.selected_session.is_some() {
            18.0
        } else {
            0.0
        };
        let available_width = (full.width() - 32.0 - ruler_width).max(120.0);
        let width = available_width.min(CHAT_COLUMN_WIDTH);
        let left = full.left() + 16.0 + (available_width - width) / 2.0;
        let card = egui::Rect::from_min_size(
            egui::pos2(left, full.top() + 8.0),
            Vec2::new(width, (full.height() - 58.0).max(70.0)),
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
        let can_send = self.state.selected_session.is_some()
            && !self.state.composer.trim().is_empty()
            && !self.state.busy;
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
        if send_now && can_send {
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
        if content.trim().is_empty() {
            return;
        }
        self.follow_tail = true;
        self.submit(BackendCommand::SendMessage {
            session_id,
            content,
        });
    }

    fn cancel_current_message(&mut self) {
        if let Some(session_id) = self.state.selected_session.clone() {
            self.submit(BackendCommand::Cancel { session_id });
        }
    }
}

impl eframe::App for EguiApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
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

    fn sidebar_frame(
        ctx: &Context,
        app: &mut EguiApp,
        size: Vec2,
        events: Vec<egui::Event>,
    ) {
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
        fn poll(&mut self, _budget: usize) -> Vec<crate::backend::BackendEvent> {
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
