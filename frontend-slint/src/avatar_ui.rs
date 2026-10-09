//! Agent avatar ("形象") dialog controller and avatar projection.
//!
//! The dialog mirrors the web agent settings panel: a paged static catalog with
//! background colors plus the global companion library, written back as the
//! canonical `icon` payload. Projection stays on the UI thread and only reads
//! cached textures; spritesheet decoding runs in the background and re-projects
//! the rows that were waiting for it.

use crate::{
    avatar_assets, companion_sprite, AvatarColorChip, AvatarDialogState, AvatarOption,
    CompanionCard, MainWindow,
};
use slint::{ComponentHandle, Color, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use wunder_desktop::NativeDesktop;
use wunder_server::worker_card_settings::{parse_agent_icon_config, AgentIconConfig};

/// Static options per page, matching the web panel's 6x4 grid.
const PAGE_SIZE: usize = 24;
/// Web `AGENT_AVATAR_COLORS`, in order.
const PALETTE: [&str; 12] = [
    "#f97316", "#ef4444", "#ec4899", "#8b5cf6", "#6366f1", "#3b82f6", "#06b6d4", "#14b8a6",
    "#10b981", "#84cc16", "#f59e0b", "#64748b",
];
/// Companion scale steps offered by the panel: 0.5 .. 1.6 by 0.1.
const SCALE_STEPS: usize = 12;
const SCALE_MIN: f32 = 0.5;

struct AvatarContext {
    api: Arc<NativeDesktop>,
    app: slint::Weak<MainWindow>,
    /// Guards against a second import picking a package while one is unpacking.
    importing: AtomicBool,
}

// The projection helpers are reached from the agent-row builder, which has no
// handle of its own, so the façade and the window live here for one install.
static CONTEXT: OnceLock<AvatarContext> = OnceLock::new();

thread_local! {
    /// Idle animation clock for the dialog preview; one timer for the whole app.
    static PREVIEW_TIMER: RefCell<slint::Timer> = RefCell::new(slint::Timer::default());
}

/// Frame index of the running idle animation; only the UI thread touches it.
static PREVIEW_FRAME: AtomicUsize = AtomicUsize::new(0);

fn context() -> Option<&'static AvatarContext> {
    CONTEXT.get()
}

/// Resolved avatar visual for one agent row. Companion sheets that are not
/// cached yet fall back to the glyph bubble and re-project once decoded.
pub(crate) fn avatar_visual(config: &AgentIconConfig) -> slint::Image {
    if config.is_companion() {
        return companion_frame(&config.id, 0);
    }
    if config.name == avatar_assets::INITIAL_KEY {
        return slint::Image::default();
    }
    avatar_assets::static_avatar_image(&config.name)
}

/// Re-read every agent row's avatar and refresh the chat agent list. Called
/// after a companion sheet finishes decoding on a background thread.
fn reproject_agents(app: &MainWindow) {
    let rows = app.get_agents();
    let mut changed = false;
    for index in 0..rows.row_count() {
        let Some(mut row) = rows.row_data(index) else {
            continue;
        };
        let config = parse_agent_icon_config(Some(row.icon_config.as_str()));
        let image = avatar_visual(&config);
        if image != row.icon_image {
            row.icon_image = image;
            rows.set_row_data(index, row);
            changed = true;
        }
    }
    if changed {
        crate::navigation_ui::project(app);
    }
}

fn companion_frame(id: &str, frame: usize) -> slint::Image {
    if companion_sprite::has_idle_frames(id) {
        return companion_sprite::idle_frame(id, frame);
    }
    schedule_companion_load(id);
    slint::Image::default()
}

/// Decode one companion's idle row off-thread, then re-project the rows that
/// were showing the glyph fallback.
fn schedule_companion_load(id: &str) {
    let Some(ctx) = context() else {
        return;
    };
    if !companion_sprite::mark_load_scheduled(id) {
        return;
    }
    let api = ctx.api.clone();
    let weak = ctx.app.clone();
    let id = id.to_string();
    std::thread::spawn(move || {
        let frames = api
            .companion_spritesheet(&id)
            .ok()
            .flatten()
            .and_then(|(_mime, bytes)| companion_sprite::decode_idle_frames(&bytes));
        let Some(frames) = frames else { return };
        let _ = weak.upgrade_in_event_loop(move |app| {
            if companion_sprite::store_idle_frames(&id, frames) {
                reproject_agents(&app);
                sync_companion_previews(&app);
            }
        });
    });
}

pub(crate) fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let _ = CONTEXT.set(AvatarContext {
        api,
        app: app.as_weak(),
        importing: AtomicBool::new(false),
    });
    let state = app.global::<AvatarDialogState>();
    state.set_palette(model(PALETTE.iter().map(|hex| AvatarColorChip {
        hex: (*hex).into(),
        brush: hex_color(hex),
    })));
    let weak = app.as_weak();

    state.on_open_requested({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            open_dialog(&app);
        }
    });
    state.on_set_kind({
        let weak = weak.clone();
        move |kind| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            state.set_kind(kind);
            state.set_status("".into());
            sync_preview(&app);
        }
    });
    state.on_select_option({
        let weak = weak.clone();
        move |key| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            let key = key.to_string();
            state.set_selected_key(key.clone().into());
            state.set_preview_image(static_preview_image(&key));
            state.set_preview_frame(slint::Image::default());
        }
    });
    state.on_set_color({
        let weak = weak.clone();
        move |hex| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            set_color(&state, hex.as_str());
        }
    });
    state.on_prev_page({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            show_page(&state, state.get_page().max(2) - 1);
        }
    });
    state.on_next_page({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            if state.get_page() < state.get_page_count() {
                show_page(&state, state.get_page() + 1);
            }
        }
    });
    state.on_select_companion({
        let weak = weak.clone();
        move |id| {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            let id = id.to_string();
            state.set_companion_id(id.clone().into());
            state.set_status("".into());
            state.set_preview_image(slint::Image::default());
            state.set_preview_frame(companion_frame(&id, 0));
            sync_preview(&app);
        }
    });
    state.on_import_companion({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            import_companion(&app);
        }
    });
    state.on_delete_companion(move |id| {
        delete_companion(id.as_str());
    });
    state.on_reset({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            let state = app.global::<AvatarDialogState>();
            let initial = state.get_agent_initial().to_string();
            state.set_kind(0);
            state.set_selected_key(avatar_assets::DEFAULT_AVATAR_KEY.into());
            state.set_companion_id("".into());
            state.set_companion_show(true);
            state.set_companion_hints(true);
            state.set_companion_scale_index(scale_index(1.0));
            set_color(&state, wunder_server::worker_card_settings::DEFAULT_AGENT_AVATAR_COLOR);
            show_page(&state, page_for_key(avatar_assets::DEFAULT_AVATAR_KEY));
            state.set_preview_image(static_preview_image(avatar_assets::DEFAULT_AVATAR_KEY));
            state.set_preview_frame(slint::Image::default());
            state.set_status("".into());
            state.set_agent_initial(initial.into());
            sync_preview(&app);
        }
    });
    state.on_cancel({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            close_dialog(&app);
        }
    });
    state.on_confirm({
        let weak = weak.clone();
        move || {
            let Some(app) = weak.upgrade() else { return };
            confirm_dialog(&app);
        }
    });
}

fn open_dialog(app: &MainWindow) {
    let state = app.global::<AvatarDialogState>();
    let payload = app.get_selected_agent_icon_config().to_string();
    let config = parse_agent_icon_config(Some(payload.as_str()));
    let initial = initial_letter(app.get_selected_agent_name().as_str());
    state.set_agent_initial(initial.into());
    state.set_kind(if config.is_companion() { 1 } else { 0 });
    state.set_selected_key(config.name.clone().into());
    set_color(&state, &config.color);
    state.set_companion_id(config.id.clone().into());
    state.set_companion_show(config.show);
    state.set_companion_hints(config.message_hints);
    state.set_companion_scale_index(scale_index(config.scale));
    state.set_status("".into());
    state.set_open(true);
    show_page(&state, page_for_key(&config.name));
    state.set_preview_image(if config.is_companion() {
        slint::Image::default()
    } else {
        static_preview_image(&config.name)
    });
    state.set_preview_frame(if config.is_companion() {
        companion_frame(&config.id, 0)
    } else {
        slint::Image::default()
    });
    sync_preview(app);
    load_companions();
}

fn close_dialog(app: &MainWindow) {
    app.global::<AvatarDialogState>().set_open(false);
    stop_preview_timer();
}

/// Confirm keeps the dialog's draft in the editor property only; the row and the
/// store change when the agent itself is saved, exactly like the web panel.
fn confirm_dialog(app: &MainWindow) {
    let state = app.global::<AvatarDialogState>();
    let draft = app.get_selected_agent_icon_config().to_string();
    let mut config = parse_agent_icon_config(Some(draft.as_str()));
    config.color = state.get_color().to_string();
    if state.get_kind() == 1 {
        let id = state.get_companion_id().trim().to_string();
        if id.is_empty() {
            state.set_status("请先选择一个动态形象".into());
            return;
        }
        config.kind = "companion".into();
        config.scope = "global".into();
        config.id = id;
        config.show = state.get_companion_show();
        config.message_hints = state.get_companion_hints();
        config.scale = scale_at(state.get_companion_scale_index());
    } else {
        config.kind = "static".into();
        config.name = state.get_selected_key().to_string();
        config.id.clear();
    }
    // `to_payload` is the single canonical writer of the shared web/server shape.
    app.set_selected_agent_icon_config(config.to_payload().into());
    // The editor's thumbnail and dirty tracker follow the draft the dialog owns.
    app.set_selected_agent_icon_image(avatar_visual(&config));
    crate::agent_editor::refresh_agent_dirty(app);
    close_dialog(app);
}

/// Re-apply the preview visual for the currently selected companion, so a sheet
/// that finished decoding while the dialog was open starts animating.
fn sync_companion_previews(app: &MainWindow) {
    let state = app.global::<AvatarDialogState>();
    if !state.get_open() || state.get_kind() != 1 {
        return;
    }
    let id = state.get_companion_id().to_string();
    if !id.is_empty() {
        state.set_preview_frame(companion_frame(&id, 0));
    }
    let companions = state.get_companions();
    for index in 0..companions.row_count() {
        let Some(mut card) = companions.row_data(index) else {
            continue;
        };
        let frame = companion_sprite::idle_frame(card.id.as_str(), 0);
        if frame != card.preview {
            card.preview = frame;
            // The model is shared with the view, so a row update is enough.
            companions.set_row_data(index, card);
        }
    }
    sync_preview(app);
}

/// Start or stop the idle animation for the dialog preview.
fn sync_preview(app: &MainWindow) {
    let state = app.global::<AvatarDialogState>();
    let id = if state.get_open() && state.get_kind() == 1 {
        state.get_companion_id().trim().to_string()
    } else {
        String::new()
    };
    if id.is_empty() || !companion_sprite::has_idle_frames(&id) {
        stop_preview_timer();
        return;
    }
    let weak = app.as_weak();
    PREVIEW_FRAME.store(0, Ordering::Relaxed);
    PREVIEW_TIMER.with(|slot| {
        slot.borrow().start(
            slint::TimerMode::Repeated,
            Duration::from_millis(companion_sprite::IDLE.frame_millis()),
            move || {
                let Some(app) = weak.upgrade() else { return };
                let state = app.global::<AvatarDialogState>();
                let id = state.get_companion_id().trim().to_string();
                let frames = if state.get_open() && state.get_kind() == 1 {
                    companion_sprite::idle_frame_count(&id)
                } else {
                    0
                };
                if frames == 0 {
                    stop_preview_timer();
                    return;
                }
                let index = PREVIEW_FRAME.load(Ordering::Relaxed) % frames;
                PREVIEW_FRAME.store((index + 1) % frames, Ordering::Relaxed);
                state.set_preview_frame(companion_sprite::idle_frame(&id, index));
            },
        );
    });
}

fn stop_preview_timer() {
    PREVIEW_TIMER.with(|slot| slot.borrow().stop());
}

/// The preview bubble: a companion frame wins over the static image, which wins
/// over the agent-initial glyph.
fn static_preview_image(key: &str) -> slint::Image {
    if key == avatar_assets::INITIAL_KEY {
        return slint::Image::default();
    }
    avatar_assets::static_avatar_image(key)
}

fn option_keys() -> Vec<String> {
    let mut keys = vec![avatar_assets::INITIAL_KEY.to_string()];
    keys.extend(avatar_assets::static_avatar_keys().map(str::to_string));
    keys
}

fn page_for_key(key: &str) -> i32 {
    let keys = option_keys();
    let position = keys.iter().position(|candidate| candidate == key).unwrap_or(0);
    (position / PAGE_SIZE + 1) as i32
}

fn show_page(state: &AvatarDialogState, page: i32) {
    let keys = option_keys();
    let pages = keys.len().div_ceil(PAGE_SIZE) as i32;
    let page = page.clamp(1, pages);
    let start = (page as usize - 1) * PAGE_SIZE;
    let options = keys[start..(start + PAGE_SIZE).min(keys.len())]
        .iter()
        .map(|key| AvatarOption {
            key: key.clone().into(),
            image: static_preview_image(key),
        })
        .collect::<Vec<_>>();
    state.set_page(page);
    state.set_page_count(pages.max(1));
    state.set_options(model(options));
}

fn load_companions() {
    let Some(ctx) = context() else { return };
    let api = ctx.api.clone();
    let weak = ctx.app.clone();
    std::thread::spawn(move || {
        let result = api.list_companions();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let state = app.global::<AvatarDialogState>();
            match result {
                Ok(records) => {
                    let cards = records
                        .into_iter()
                        .map(|record| CompanionCard {
                            id: record.id.clone().into(),
                            glyph: initial_letter(&record.display_name).into(),
                            name: record.display_name.into(),
                            description: record.description.into(),
                            preview: companion_sprite::idle_frame(&record.id, 0),
                        })
                        .collect::<Vec<_>>();
                    let count = cards.len();
                    state.set_companions(model(cards));
                    if count > 0 {
                        // Warm the cache for the visible rows so the library and
                        // the agent list animate without a second round-trip.
                        for id in state.get_companions().iter().take(4).map(|card| card.id.to_string()) {
                            companion_frame(id.as_str(), 0);
                        }
                    }
                }
                Err(error) => state.set_status(format!("形象库读取失败：{error}").into()),
            }
        });
    });
}

fn import_companion(app: &MainWindow) {
    let Some(ctx) = context() else {
        return;
    };
    if ctx.importing.load(Ordering::SeqCst) {
        return;
    }
    let Some(path) = crate::file_dialog::pick_companion_package() else {
        return;
    };
    let state = app.global::<AvatarDialogState>();
    state.set_status("正在导入形象包…".into());
    ctx.importing.store(true, Ordering::SeqCst);
    let api = ctx.api.clone();
    let weak = ctx.app.clone();
    let importing = &ctx.importing;
    std::thread::spawn(move || {
        let result = api.import_companion(std::path::Path::new(&path));
        let _ = weak.upgrade_in_event_loop(move |app| {
            importing.store(false, Ordering::SeqCst);
            let state = app.global::<AvatarDialogState>();
            match result {
                Ok(record) => {
                    state.set_status("".into());
                    let id = record.id;
                    state.set_kind(1);
                    state.set_companion_id(id.clone().into());
                    state.set_preview_image(slint::Image::default());
                    state.set_preview_frame(companion_frame(&id, 0));
                    sync_preview(&app);
                    load_companions();
                }
                Err(error) => state.set_status(format!("形象包导入失败：{error}").into()),
            }
        });
    });
}

fn delete_companion(id: &str) {
    let Some(ctx) = context() else { return };
    let id = id.to_string();
    let api = ctx.api.clone();
    let weak = ctx.app.clone();
    std::thread::spawn(move || {
        let result = api.delete_companion(&id);
        let _ = weak.upgrade_in_event_loop(move |app| {
            let state = app.global::<AvatarDialogState>();
            match result {
                Ok(_) => {
                    companion_sprite::forget(id.as_str());
                    crate::companion_pet::forget(id.as_str());
                    if state.get_companion_id() == id.as_str() {
                        state.set_companion_id("".into());
                        state.set_preview_frame(slint::Image::default());
                        stop_preview_timer();
                    }
                    state.set_status("".into());
                    reproject_agents(&app);
                    load_companions();
                }
                Err(error) => state.set_status(format!("形象删除失败：{error}").into()),
            }
        });
    });
}

fn set_color(state: &AvatarDialogState, hex: &str) {
    let normalized = if PALETTE.contains(&hex) {
        hex.to_string()
    } else {
        wunder_server::worker_card_settings::DEFAULT_AGENT_AVATAR_COLOR.to_string()
    };
    state.set_color(normalized.clone().into());
    state.set_color_brush(hex_color(&normalized));
}

fn hex_color(hex: &str) -> Color {
    let value = hex.trim().trim_start_matches('#');
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(value.get(range).unwrap_or("00"), 16).unwrap_or(0)
    };
    if value.len() == 6 {
        Color::from_rgb_u8(channel(0..2), channel(2..4), channel(4..6))
    } else {
        Color::from_rgb_u8(59, 130, 246)
    }
}

fn scale_at(index: i32) -> f32 {
    let index = index.clamp(0, SCALE_STEPS as i32 - 1) as usize;
    (SCALE_MIN * 10.0 + index as f32) / 10.0
}

fn scale_index(scale: f32) -> i32 {
    let steps = ((scale.clamp(SCALE_MIN, 1.6) - SCALE_MIN) * 10.0).round();
    steps.clamp(0.0, (SCALE_STEPS - 1) as f32) as i32
}

/// Web `resolveAgentAvatarInitial`: the first letter, uppercased.
pub(crate) fn initial_letter(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".to_string())
}

fn model<T: Clone + 'static>(rows: impl IntoIterator<Item = T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows.into_iter().collect::<Vec<_>>()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_cover_the_web_catalog() {
        // `initial` + 59 static faces = 3 pages of 24. The gaps in the key
        // sequence (avatar-000…avatar-083) shift the default onto page 2.
        assert_eq!(option_keys().len(), 60);
        assert_eq!(page_for_key(avatar_assets::INITIAL_KEY), 1);
        assert_eq!(page_for_key(avatar_assets::DEFAULT_AVATAR_KEY), 2);
        assert_eq!(page_for_key("avatar-083"), 3);
    }

    #[test]
    fn scale_round_trips_on_panel_steps() {
        assert_eq!(scale_at(5), 1.0);
        assert_eq!(scale_index(1.0), 5);
        assert_eq!(scale_index(0.2), 0);
        assert_eq!(scale_at(scale_index(2.4)), 1.6);
    }

    #[test]
    fn palette_colors_parse() {
        for hex in PALETTE {
            assert_ne!(hex_color(hex), Color::from_rgb_u8(0, 0, 0));
        }
        assert_eq!(hex_color("#3b82f6"), Color::from_rgb_u8(59, 130, 246));
        assert_eq!(hex_color("bad"), Color::from_rgb_u8(59, 130, 246));
    }
}
