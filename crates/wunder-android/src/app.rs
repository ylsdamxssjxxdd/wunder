//! UI assembly and the 33 ms projection tick.
//!
//! Slint runs on the main thread; the engine owns everything blocking. The
//! engine publishes plain `RowData` (Send), and this module converts it to the
//! Slint row model, caching each row by content signature so a streaming token
//! only re-projects the live tail.

use crate::engine::{self, Command, Shared};
use crate::md::MarkdownRenderer;
use crate::project::RowData;
use crate::{MobileSession, MobileWindow, ModelCard, Theme, TimelineRow};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(33);
const CACHE_LIMIT: usize = 800;

pub fn build() -> MobileWindow {
    let (ui, model) = create_window();
    // The engine outlives this window; a recreated activity attaches to the
    // same transcript and any turn that is still streaming.
    let handle = engine::acquire();
    let shared = handle.shared.clone();
    let commands = handle.command_sender();
    wire_callbacks(&ui, &commands, &shared);
    start_tick(&ui, &shared, &model, commands);
    ui
}

thread_local! {
    // The tick timer belongs to one native activity thread. Android runs every
    // activity instance on its own native thread, so a thread-local slot drops
    // the previous timer when that activity goes away instead of leaking one
    // timer per recreation.
    static TICK_TIMER: std::cell::RefCell<Option<Timer>> = const { std::cell::RefCell::new(None) };
}

/// Window without the engine, used by the desktop preview binary. The layout
/// and every shared component are identical to the phone build.
pub fn build_preview() -> MobileWindow {
    let (ui, model) = create_window();
    install_preview_data(&ui, &model);
    let model_for_toggle = model.clone();
    ui.on_toggle_row(move |index| {
        let index = index.max(0) as usize;
        if let Some(mut row) = model_for_toggle.row_data(index) {
            if row.foldable {
                row.open = !row.open;
                model_for_toggle.set_row_data(index, row);
            }
        }
    });
    ui
}

fn create_window() -> (MobileWindow, Rc<VecModel<TimelineRow>>) {
    crate::fonts::install();
    let ui = MobileWindow::new().expect("create the mobile window");
    let model = Rc::new(VecModel::<TimelineRow>::default());
    ui.set_rows(ModelRc::from(model.clone()));
    ui.set_greeting("".into());
    (ui, model)
}

fn wire_callbacks(
    ui: &MobileWindow,
    commands: &std::sync::mpsc::Sender<Command>,
    shared: &Arc<Mutex<Shared>>,
) {
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_drawer(move || {
        let _ = sender.send(Command::RefreshSessions);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(true);
        }
    });
    let weak = ui.as_weak();
    ui.on_close_drawer(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_select_session(move |id| {
        let _ = sender.send(Command::Select(id.to_string()));
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_new_session(move || {
        let _ = sender.send(Command::NewThread);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_send(move || {
        let Some(ui) = weak.upgrade() else { return };
        let draft = ui.get_draft().to_string();
        let draft = draft.trim().to_string();
        if draft.is_empty() {
            return;
        }
        let _ = sender.send(Command::Send(draft));
        ui.set_draft("".into());
    });

    let sender = commands.clone();
    ui.on_stop(move || {
        let _ = sender.send(Command::Stop);
    });

    let sender = commands.clone();
    ui.on_select_model(move |_index, key| {
        let _ = sender.send(Command::SelectModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_reasoning_effort_changed(move |value| {
        let _ = sender.send(Command::ReasoningEffort(value.to_string()));
    });

    // ---- settings sheet ----------------------------------------------
    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_settings(move |page| {
        log::info!("open settings page={page}");
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::RefreshUbuntu);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_settings_page(page);
            ui.set_settings_open(true);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_close_settings(move || {
        log::info!("close settings");
        let _ = sender.send(Command::CloseModelEditor);
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_open(false);
        }
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_theme(move |theme| {
        let (send_key, font_size) = match weak.upgrade() {
            Some(ui) => (
                ui.get_settings_send_key().to_string(),
                ui.get_settings_font_size(),
            ),
            None => return,
        };
        let _ = sender.send(Command::SavePreferences {
            theme: theme.to_string(),
            send_key,
            font_size,
        });
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_font_size(move |font_size| {
        let Some(ui) = weak.upgrade() else { return };
        let font_size = font_size.clamp(12, 20);
        // The persisted value drives the live scale; applying it here keeps
        // the stepper responsive while the engine writes the preference.
        ui.global::<Theme>().set_font_scale(font_size as f32 / 14.0);
        let _ = sender.send(Command::SavePreferences {
            theme: ui.get_settings_theme().to_string(),
            send_key: ui.get_settings_send_key().to_string(),
            font_size,
        });
    });

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_save_send_key(move |send_key| {
        let Some(ui) = weak.upgrade() else { return };
        let _ = sender.send(Command::SavePreferences {
            theme: ui.get_settings_theme().to_string(),
            send_key: send_key.to_string(),
            font_size: ui.get_settings_font_size(),
        });
    });

    let sender = commands.clone();
    ui.on_save_language(move |language| {
        let _ = sender.send(Command::SaveLanguage(language.to_string()));
    });

    let sender = commands.clone();
    ui.on_install_ubuntu(move || {
        let _ = sender.send(Command::InstallUbuntu);
    });

    let sender = commands.clone();
    ui.on_set_ubuntu_enabled(move |enabled| {
        let _ = sender.send(Command::SetUbuntuEnabled(enabled));
    });

    let sender = commands.clone();
    ui.on_open_model_editor(move |key| {
        let key = key.to_string();
        let _ = sender.send(Command::OpenModelEditor {
            key: if key.is_empty() { None } else { Some(key) },
        });
    });

    let sender = commands.clone();
    ui.on_close_model_editor(move || {
        let _ = sender.send(Command::CloseModelEditor);
    });

    let sender = commands.clone();
    ui.on_save_model(move |card, secret| {
        let _ = sender.send(Command::SaveModel {
            card,
            secret: secret.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_probe_model(move |card, secret| {
        let _ = sender.send(Command::ProbeModel {
            card,
            secret: secret.to_string(),
        });
    });

    let sender = commands.clone();
    ui.on_delete_model(move |key| {
        let _ = sender.send(Command::DeleteModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_set_default_model(move |key| {
        let _ = sender.send(Command::SetDefaultModel(key.to_string()));
    });

    let sender = commands.clone();
    ui.on_permission_changed(move |value| {
        let _ = sender.send(Command::PermissionMode(value.to_string()));
    });
    ui.on_remove_attachment(|_index| {});

    let sender = commands.clone();
    let weak = ui.as_weak();
    ui.on_open_model_settings(move || {
        log::info!("open model settings");
        let _ = sender.send(Command::LoadSettings);
        let _ = sender.send(Command::RefreshUbuntu);
        if let Some(ui) = weak.upgrade() {
            ui.set_drawer_open(false);
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
    });

    // ---- row fold ----------------------------------------------------
    let shared_for_toggle = shared.clone();
    ui.on_toggle_row(move |index| {
        let Ok(mut state) = shared_for_toggle.lock() else {
            return;
        };
        // Live rows carry -1 (`payload`): their indices are not stable while
        // the durable prefix grows, so a tap must not fold a durable row.
        if index < 0 {
            return;
        }
        let index = index as usize;
        let Some(row) = state.durable_rows.get_mut(index) else {
            return;
        };
        if !row.foldable {
            return;
        }
        row.open = !row.open;
        state.patch_row = Some(index);
        state.patch_rev = state.patch_rev.wrapping_add(1);
    });

    // The transcript's copy buttons and the message selection both land here;
    // the Android backend forwards the text to the system clipboard. Give the
    // tap visible feedback: the system clipboard has no toast of its own in a
    // NativeActivity window.
    let weak = ui.as_weak();
    let shared_for_copy = shared.clone();
    ui.on_copy_text(move |text| {
        let Some(ui) = weak.upgrade() else { return };
        i_slint_core::window::WindowInner::from_pub(ui.window())
            .context()
            .platform()
            .set_clipboard_text(
                text.as_str(),
                i_slint_core::platform::Clipboard::DefaultClipboard,
            );
        if let Ok(mut state) = shared_for_copy.lock() {
            state.notify("已复制");
            state.touch();
        }
    });
}

/// Caches converted rows by id + content signature. Only the changed tail is
/// re-projected while a turn streams.
struct RowCache {
    renderer: MarkdownRenderer,
    rows: HashMap<String, (u64, TimelineRow)>,
}

impl Default for RowCache {
    fn default() -> Self {
        Self {
            renderer: MarkdownRenderer::new(),
            rows: HashMap::new(),
        }
    }
}

impl RowCache {
    fn row(&mut self, data: &RowData) -> TimelineRow {
        let signature = data.signature();
        if let Some((cached_signature, row)) = self.rows.get(&data.id) {
            if *cached_signature == signature {
                return row.clone();
            }
        }
        if self.rows.len() >= CACHE_LIMIT {
            self.rows.clear();
        }
        let row = data.to_row(&mut self.renderer);
        self.rows
            .insert(data.id.clone(), (signature, row.clone()));
        row
    }

    fn row_items(&mut self, data: &[RowData]) -> Vec<TimelineRow> {
        data.iter().map(|row| self.row(row)).collect()
    }
}

/// True once the durable projection carries the live turn's body or error.
/// From then on the durable rows are the complete turn and the live tail is
/// dropped instead of rendered twice.
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

/// Same normalization as 蛹: unknown or legacy theme values fall back to the
/// default accent instead of leaving the palette unset.
fn normalize_palette(theme: &str) -> &'static str {
    match theme.trim() {
        "hula-green" => "hula-green",
        "forest" => "forest",
        "honey" => "honey",
        "minimal" => "minimal",
        "tech-blue" => "tech-blue",
        _ => "terracotta",
    }
}

fn start_tick(
    ui: &MobileWindow,
    shared: &Arc<Mutex<Shared>>,
    model: &Rc<VecModel<TimelineRow>>,
    commands: std::sync::mpsc::Sender<Command>,
) {
    let weak = ui.as_weak();
    let shared = shared.clone();
    let model = model.clone();
    let mut ubuntu_ticks = 0u32;
    let mut cache = RowCache::default();
    let mut last_rev = 0u64;
    let mut last_rows = 0u64;
    let mut last_live = 0u64;
    let mut last_toast = 0u64;
    let mut toast_deadline: Option<Instant> = None;
    let mut last_patch = 0u64;
    let mut last_sessions = 0u64;
    let mut last_models = 0u64;
    let mut last_settings = 0u64;
    let mut last_ubuntu = 0u64;
    let mut last_editor = 0u64;
    let mut last_probe = 0u64;
    let mut editor_was_open = false;
    let mut last_ime = -1.0f32;

    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || {
        let Some(ui) = weak.upgrade() else { return };
        // The window is edge-to-edge: the activity does not resize under the
        // soft keyboard, so the shell reads Slint's virtual-keyboard rect and
        // lifts the transcript and composer above it.
        let ime = ui
            .window()
            .virtual_keyboard(i_slint_core::InternalToken)
            .map(|(_origin, size)| size.height)
            .unwrap_or(0.0);
        if (ime - last_ime).abs() > 0.5 {
            ui.set_ime_height(ime);
            last_ime = ime;
        }
        let Ok(state) = shared.lock() else { return };

        if state.rev != last_rev {
            ui.set_thread_title(state.title.as_str().into());
            ui.set_thread_subtitle(state.subtitle.as_str().into());
            ui.set_workspace_name(state.workspace.as_str().into());
            ui.set_connection_state(state.connection.as_str().into());
            ui.set_generating(state.generating);
            ui.set_stopping(state.stopping);
            ui.set_loading(state.loading);
            ui.set_spinning(state.generating || state.loading);
            ui.set_model_name(state.model_name.as_str().into());
            ui.set_model_key(state.model_key.as_str().into());
            ui.set_send_key(state.send_key.as_str().into());
            ui.set_stream_revision(state.stream_revision);
            last_rev = state.rev;
        }

        if state.sessions_rev != last_sessions {
            ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(
                state.sessions.clone(),
            ))));
            last_sessions = state.sessions_rev;
        }

        if state.models_rev != last_models {
            ui.set_models(ModelRc::from(Rc::new(VecModel::from(state.models.clone()))));
            ui.set_model_catalogue_count(state.models.len() as i32);
            last_models = state.models_rev;
        }

        if state.ubuntu.state == "installing" {
            // Java owns extraction progress; poll it at 1 Hz instead of
            // touching JNI from the 33 ms render tick.
            ubuntu_ticks = ubuntu_ticks.wrapping_add(1);
            if ubuntu_ticks % 30 == 0 {
                let _ = commands.send(Command::RefreshUbuntu);
            }
        }

        if state.settings_rev != last_settings {
            ui.set_settings_theme(state.settings.theme.as_str().into());
            ui.set_settings_font_size(state.settings.font_size);
            ui.set_settings_send_key(state.settings.send_key.as_str().into());
            ui.set_settings_language(state.settings.language.as_str().into());
            ui.set_settings_workspace(state.settings.workspace_root.as_str().into());
            ui.set_settings_version(state.settings.version.as_str().into());
            let theme = ui.global::<Theme>();
            theme.set_palette(normalize_palette(&state.settings.theme).into());
            theme.set_font_scale(state.settings.font_size.clamp(12, 20) as f32 / 14.0);
            last_settings = state.settings_rev;
        }

        if state.ubuntu_rev != last_ubuntu {
            ui.set_settings_ubuntu_state(state.ubuntu.state.as_str().into());
            ui.set_settings_ubuntu_progress(state.ubuntu.progress);
            ui.set_settings_ubuntu_stage(state.ubuntu.stage.as_str().into());
            ui.set_settings_ubuntu_version(state.ubuntu.version.as_str().into());
            ui.set_settings_ubuntu_error(state.ubuntu.error.as_str().into());
            ui.set_settings_ubuntu_enabled(state.ubuntu.enabled);
            last_ubuntu = state.ubuntu_rev;
        }

        if state.editor_rev != last_editor {
            if let Some(editor) = &state.editor {
                ui.set_editor_key(editor.original_key.as_str().into());
                ui.set_editor_new(editor.original_key.is_empty());
                ui.set_editor_default(editor.is_default);
                ui.set_editor_error(editor.error.as_str().into());
                ui.set_editor_probe(editor.probe_message.as_str().into());
                ui.set_editor_probing(editor.probing);
                if !editor_was_open {
                    // Seed exactly once per opening. Later revisions carry a
                    // save error or a probe result and must never overwrite
                    // what the user has typed since.
                    ui.set_editor_draft(editor.card.clone());
                    ui.set_editor_secret("".into());
                    ui.set_editor_probed_context("".into());
                }
                ui.set_editor_open(true);
                editor_was_open = true;
            } else {
                ui.set_editor_open(false);
                editor_was_open = false;
            }
            last_editor = state.editor_rev;
        }

        if state.probe_rev != last_probe {
            ui.set_editor_probed_context(state.probed_context.as_str().into());
            last_probe = state.probe_rev;
        }

        if state.toast_rev != last_toast {
            ui.set_toast(state.toast.as_str().into());
            last_toast = state.toast_rev;
            toast_deadline = Some(Instant::now() + Duration::from_millis(2600));
        } else if toast_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            // The engine keeps the message for late ticks; the shell owns the
            // dismissal so a toast never parks over the next interaction.
            ui.set_toast("".into());
            toast_deadline = None;
        }

        if state.rows_rev != last_rows || state.live_rev != last_live {
            // The durable watcher and the live stream can describe the same
            // turn. Drop exact duplicates (the user bubble, an error card) as
            // soon as they exist in both, and once the durable projection has
            // the turn's body or error the live rows are redundant entirely.
            let live: Vec<RowData> = if state.live_rows.is_empty() {
                Vec::new()
            } else if durable_has_body(&state.durable_rows, &state.live_root) {
                Vec::new()
            } else {
                let durable_ids: std::collections::HashSet<&str> =
                    state.durable_rows.iter().map(|row| row.id.as_str()).collect();
                state
                    .live_rows
                    .iter()
                    .filter(|row| !durable_ids.contains(row.id.as_str()))
                    .cloned()
                    .collect()
            };
            if state.rows_rev != last_rows {
                let mut rows = Vec::with_capacity(state.durable_rows.len() + live.len());
                rows.extend(cache.row_items(&state.durable_rows));
                rows.extend(cache.row_items(&live));
                model.set_vec(rows);
                last_rows = state.rows_rev;
            } else {
                let durable_len = state.durable_rows.len();
                let total = durable_len + live.len();
                let have = model.row_count();
                let rows = cache.row_items(&live);
                for index in durable_len..total.min(have) {
                    model.set_row_data(index, rows[index - durable_len].clone());
                }
                for index in have..total {
                    model.push(rows[index - durable_len].clone());
                }
                while model.row_count() > total {
                    model.remove(total);
                }
            }
            last_live = state.live_rev;
            last_patch = state.patch_rev;
        } else if state.patch_rev != last_patch {
            if let Some(index) = state.patch_row {
                if let Some(data) = state.durable_rows.get(index) {
                    model.set_row_data(index, cache.row(data));
                }
            }
            last_patch = state.patch_rev;
        }
    });
    // Slint cancels a dropped timer, so it must outlive `build`.
    TICK_TIMER.with(|slot| {
        *slot.borrow_mut() = Some(timer);
    });
}

/// Seed the window with a representative transcript for layout review on a
/// desktop (the `wunder-android-preview` binary). Kept next to the real
/// projection so preview rows go through the same renderer.
pub fn install_preview_data(ui: &MobileWindow, model: &Rc<VecModel<TimelineRow>>) {
    use crate::project::{KIND_BODY, KIND_ERROR, KIND_FOOTER, KIND_REASON, KIND_TOOL, KIND_USER};

    let sessions = vec![
        MobileSession {
            id: "preview-1".into(),
            title: "移动端聊天区对齐".into(),
            preview: "心舰".into(),
            time: "11-06 14:20".into(),
            status: "running".into(),
            active: true,
        },
        MobileSession {
            id: "preview-2".into(),
            title: "检查无障碍控制链路".into(),
            preview: "心舰".into(),
            time: "11-05 09:12".into(),
            status: "idle".into(),
            active: false,
        },
    ];
    ui.set_sessions(ModelRc::from(Rc::new(VecModel::from(sessions))));
    ui.set_thread_title("移动端聊天区对齐".into());
    ui.set_thread_subtitle("running · 心舰".into());
    ui.set_workspace_name("心舰".into());
    ui.set_connection_state("本地引擎".into());
    ui.set_model_name("预览模型".into());
    ui.set_model_key("preview-chat".into());

    // Settings-page fixtures: enough rows to review the index, the catalogue
    // and the editor on a workstation without starting the engine.
    ui.set_settings_theme("terracotta".into());
    ui.set_settings_font_size(14);
    ui.set_settings_send_key("enter".into());
    ui.set_settings_language("zh-CN".into());
    ui.set_settings_workspace("应用私有目录 / workspaces".into());
    ui.set_settings_version(env!("CARGO_PKG_VERSION").into());
    ui.set_settings_ubuntu_state("installing".into());
    ui.set_settings_ubuntu_progress(42);
    ui.set_settings_ubuntu_stage("正在解压 Ubuntu 根文件系统…（4200 项）".into());
    ui.set_settings_ubuntu_enabled(true);
    let models = vec![
        preview_model_card("preview-chat", "llm", "预览对话模型", "https://api.example.com/v1", true),
        preview_model_card("preview-speech", "tts", "预览语音模型", "https://api.example.com/v1", false),
    ];
    ui.set_models(ModelRc::from(Rc::new(VecModel::from(models))));
    ui.set_model_catalogue_count(2);

    let mut rows: Vec<RowData> = Vec::new();
    let mut user = RowData::new(KIND_USER, "p-user");
    user.text = "把聊天区搬到手机上，样式和蛹保持一致。".into();
    user.time = "14:20".into();
    rows.push(user);

    let mut reason = RowData::new(KIND_REASON, "p-reason");
    reason.tool_name = "思考".into();
    reason.summary = "先确认聊天组件的复用边界，再处理单窗口约束。".into();
    reason.detail = "聊天区必须直接 import 蛹的 composer 与 timeline，避免出现第二套视觉。".into();
    reason.reasoning_src = reason.detail.clone();
    reason.foldable = true;
    rows.push(reason);

    let mut tool = RowData::new(KIND_TOOL, "p-tool");
    tool.tool_name = "读取文件".into();
    tool.summary = "frontend-slint/ui/timeline.slint".into();
    tool.tool_metrics = "2.4k 0.8s".into();
    tool.foldable = true;
    rows.push(tool);

    let mut body = RowData::new(KIND_BODY, "p-body");
    let text = "聊天区现在复用了蛹的同一套组件：\n\n- `TimelineView` 负责消息流\n- `Composer` 负责输入与模型选择\n\n代码块样式也保持一致：\n\n```rust\nlet rows = project::project_turn(&turn);\n```\n\n> 手机上只保留单窗口与抽屉。";
    body.text = text.into();
    body.blocks_src = text.into();
    rows.push(body);

    let mut error = RowData::new(KIND_ERROR, "p-error");
    error.error_title = "执行失败".into();
    error.text = "LLM unavailable: LLM request failed".into();
    error.status_kind = 2;
    rows.push(error);

    let mut footer = RowData::new(KIND_FOOTER, "p-footer");
    footer.text = text.into();
    footer.stats = vec![
        ("stopwatch".into(), "3.2s".into()),
        ("bolt".into(), "1.8k".into()),
    ];
    rows.push(footer);

    for (index, row) in rows.iter_mut().enumerate() {
        row.payload = index as i32;
    }
    let mut renderer = MarkdownRenderer::new();
    model.set_vec(
        rows.iter()
            .map(|row| row.to_row(&mut renderer))
            .collect::<Vec<TimelineRow>>(),
    );
    ui.set_stream_revision(1);
}

fn preview_model_card(
    key: &str,
    kind: &str,
    name: &str,
    base_url: &str,
    is_default: bool,
) -> ModelCard {
    let mut card = ModelCard::default();
    card.key = key.into();
    card.provider = "openai_compatible".into();
    card.model = name.into();
    card.base_url = base_url.into();
    card.model_type = kind.into();
    card.is_default = is_default;
    card.temperature = "0.7".into();
    card.timeout_s = "120".into();
    card.max_output = "32768".into();
    card.max_context = "128000".into();
    card.tool_call_mode = "function_call".into();
    card.support_vision = true;
    card.stream_include_usage = true;
    card
}

/// Opens one of the settings pages in the desktop preview so layout review can
/// reach the sheet without an engine: `settings`, `models` or `editor`.
pub fn open_preview_page(ui: &MobileWindow, page: &str) {
    match page {
        "settings" => {
            ui.set_settings_page(0);
            ui.set_settings_open(true);
        }
        "models" => {
            ui.set_settings_page(1);
            ui.set_settings_open(true);
        }
        "editor" => {
            let card = preview_model_card(
                "preview-chat",
                "llm",
                "预览对话模型",
                "https://api.example.com/v1",
                true,
            );
            ui.set_settings_page(1);
            ui.set_settings_open(true);
            ui.set_editor_key(card.key.clone());
            ui.set_editor_new(false);
            ui.set_editor_default(true);
            ui.set_editor_draft(card);
            ui.set_editor_secret("".into());
            ui.set_editor_probe("探测成功：上下文 128000".into());
            ui.set_editor_open(true);
            // The preview window is not running yet, so the sheet's
            // `changed editor-open` handler has not been installed; select
            // the editor page explicitly.
            ui.set_settings_page(2);
        }
        _ => {}
    }
}
