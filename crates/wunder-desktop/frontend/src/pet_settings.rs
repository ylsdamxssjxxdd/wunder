//! Desktop pet settings controller: the desktop counterpart of the web
//! `companionStore` settings page. One globally selected companion drives the
//! floating pet (see [`crate::companion_pet`]) and is fully decoupled from the
//! agent list. The library loads once on start, so the settings page shows the
//! global companions immediately, exactly like the web page.

use crate::{avatar_ui, companion_sprite, CompanionCard, MainWindow, PetSettingsState};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};
use wunder_desktop::{native::clamp_companion_scale, NativeDesktop};

/// Web `LIBRARY_PAGE_SIZE`: six cards per library page.
const PAGE_SIZE: usize = 6;

/// The settings facade, kept for callbacks that carry no handle of their own.
static API: OnceLock<Arc<NativeDesktop>> = OnceLock::new();

fn api() -> Option<&'static Arc<NativeDesktop>> {
    API.get()
}

/// One library row, held plain so pagination never re-hits the server.
#[derive(Clone)]
struct LibItem {
    id: String,
    name: String,
    description: String,
}

thread_local! {
    static LIBRARY: RefCell<Vec<LibItem>> = const { RefCell::new(Vec::new()) };
}

pub fn install(app: &MainWindow, api: Arc<NativeDesktop>) {
    let _ = API.set(Arc::clone(&api));
    let state = app.global::<PetSettingsState>();
    state.set_scale_label(scale_label(state.get_scale()).into());

    let weak = app.as_weak();
    state.on_toggle_enabled({
        let weak = weak.clone();
        move |value| on_toggle_enabled(&weak, value)
    });
    state.on_toggle_hints({
        let weak = weak.clone();
        move |value| on_toggle_hints(&weak, value)
    });
    state.on_set_scale({
        let weak = weak.clone();
        move |value| on_set_scale(&weak, value)
    });
    state.on_select({
        let weak = weak.clone();
        move |id| on_select(&weak, id.to_string())
    });
    state.on_refresh({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                refresh_library(&app);
            }
        }
    });
    state.on_import_package({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                import_package(&app);
            }
        }
    });
    state.on_delete({
        let weak = weak.clone();
        move |id| {
            if let Some(app) = weak.upgrade() {
                delete_companion(&app, id.to_string());
            }
        }
    });
    state.on_prev_page({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                let page = app.global::<PetSettingsState>().get_page().max(2) - 1;
                show_page(&app, page);
            }
        }
    });
    state.on_next_page({
        let weak = weak.clone();
        move || {
            if let Some(app) = weak.upgrade() {
                let state = app.global::<PetSettingsState>();
                if state.get_page() < state.get_page_count() {
                    show_page(&app, state.get_page() + 1);
                }
            }
        }
    });

    // Persisted settings first, then the library, so the page shows the selected
    // companion as soon as the list lands.
    let weak = app.as_weak();
    let api = Arc::clone(&api);
    std::thread::spawn(move || {
        let persisted = api.companion_overlay_state();
        let library = api.list_companions();
        let _ = weak.upgrade_in_event_loop(move |app| {
            let state = app.global::<PetSettingsState>();
            state.set_enabled(persisted.enabled);
            state.set_hints(persisted.message_hints);
            let scale = clamp_companion_scale(persisted.scale);
            state.set_scale(scale);
            state.set_scale_label(scale_label(scale).into());
            state.set_selected_id(persisted.selected_id.clone().into());
            match library {
                Ok(records) => store_library(records),
                Err(error) => {
                    app.global::<PetSettingsState>()
                        .set_status(format!("形象库读取失败：{error}").into());
                }
            }
            resolve_selected_name(&app);
            show_page(&app, 1);
            crate::companion_pet::adopt(&app, persisted);
        });
    });
}

/// Rebuild the visible page; called once a sprite finishes decoding.
pub(crate) fn refresh_page(app: &MainWindow) {
    let page = app.global::<PetSettingsState>().get_page();
    show_page(app, page);
}

fn on_toggle_enabled(weak: &slint::Weak<MainWindow>, value: bool) {
    let Some(app) = weak.upgrade() else { return };
    app.global::<PetSettingsState>().set_enabled(value);
    apply_and_persist(&app);
}

fn on_toggle_hints(weak: &slint::Weak<MainWindow>, value: bool) {
    let Some(app) = weak.upgrade() else { return };
    app.global::<PetSettingsState>().set_hints(value);
    apply_and_persist(&app);
}

fn on_set_scale(weak: &slint::Weak<MainWindow>, value: f32) {
    let Some(app) = weak.upgrade() else { return };
    let scale = clamp_companion_scale(value);
    let state = app.global::<PetSettingsState>();
    state.set_scale(scale);
    state.set_scale_label(scale_label(scale).into());
    apply_and_persist(&app);
}

fn on_select(weak: &slint::Weak<MainWindow>, id: String) {
    let Some(app) = weak.upgrade() else { return };
    let state = app.global::<PetSettingsState>();
    state.set_selected_id(id.clone().into());
    let name = LIBRARY.with(|slot| {
        slot.borrow()
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| id.clone())
    });
    state.set_selected_name(name.into());
    state.set_status("".into());
    apply_and_persist(&app);
}

/// Persist the page's settings, then adopt the refreshed overlay state so the
/// floating pet reflects the change at once.
fn apply_and_persist(app: &MainWindow) {
    let Some(api) = api().cloned() else { return };
    let state = app.global::<PetSettingsState>();
    let enabled = state.get_enabled();
    let hints = state.get_hints();
    let scale = state.get_scale();
    let selected = state.get_selected_id().to_string();
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.save_companion_settings(enabled, &selected, scale, hints);
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(snapshot) => crate::companion_pet::adopt(&app, snapshot),
            Err(_) => crate::companion_pet::sync(&app),
        });
    });
}

fn refresh_library(app: &MainWindow) {
    let Some(api) = api().cloned() else { return };
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.list_companions();
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(records) => {
                store_library(records);
                resolve_selected_name(&app);
                show_page(&app, 1);
            }
            Err(error) => app
                .global::<PetSettingsState>()
                .set_status(format!("形象库读取失败：{error}").into()),
        });
    });
}

fn import_package(app: &MainWindow) {
    let Some(api) = api().cloned() else { return };
    let Some(path) = crate::file_dialog::pick_companion_package() else {
        return;
    };
    app.global::<PetSettingsState>()
        .set_status("正在导入形象包…".into());
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.import_companion(std::path::Path::new(&path));
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => {
                app.global::<PetSettingsState>().set_status("".into());
                refresh_library(&app);
            }
            Err(error) => app
                .global::<PetSettingsState>()
                .set_status(format!("形象包导入失败：{error}").into()),
        });
    });
}

fn delete_companion(app: &MainWindow, id: String) {
    let Some(api) = api().cloned() else { return };
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let result = api.delete_companion(&id);
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(_) => {
                companion_sprite::forget(&id);
                crate::companion_pet::forget(&id);
                let state = app.global::<PetSettingsState>();
                if state.get_selected_id() == id.as_str() {
                    state.set_selected_id("".into());
                    state.set_selected_name("".into());
                }
                state.set_status("".into());
                refresh_library(&app);
            }
            Err(error) => app
                .global::<PetSettingsState>()
                .set_status(format!("形象删除失败：{error}").into()),
        });
    });
}

fn store_library(
    records: impl IntoIterator<Item = wunder_desktop::native::CompanionSummaryRecord>,
) {
    let items = records
        .into_iter()
        .map(|record| LibItem {
            id: record.id.to_string(),
            name: record.display_name.to_string(),
            description: record.description.to_string(),
        })
        .collect::<Vec<_>>();
    LIBRARY.with(|slot| *slot.borrow_mut() = items);
}

fn resolve_selected_name(app: &MainWindow) {
    let state = app.global::<PetSettingsState>();
    let id = state.get_selected_id().to_string();
    if id.trim().is_empty() {
        state.set_selected_name("".into());
        return;
    }
    let name = LIBRARY.with(|slot| {
        slot.borrow()
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.name.clone())
    });
    if let Some(name) = name {
        state.set_selected_name(name.into());
    }
}

fn show_page(app: &MainWindow, page: i32) {
    let items = LIBRARY.with(|slot| slot.borrow().clone());
    let pages = items.len().div_ceil(PAGE_SIZE).max(1) as i32;
    let page = page.clamp(1, pages);
    let start = (page as usize - 1) * PAGE_SIZE;
    let cards = items
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|item| CompanionCard {
            id: item.id.clone().into(),
            glyph: avatar_ui::initial_letter(&item.name).into(),
            name: item.name.clone().into(),
            description: item.description.clone().into(),
            // Also warms the sprite cache; the decode completion re-renders us.
            preview: avatar_ui::companion_frame(&item.id, 0),
        })
        .collect::<Vec<_>>();
    let state = app.global::<PetSettingsState>();
    state.set_companions(model(cards));
    state.set_library_count(items.len() as i32);
    state.set_page(page);
    state.set_page_count(pages);
    state.set_page_label(page_label(page, pages).into());
}

fn scale_label(scale: f32) -> String {
    format!("{scale:.1}x")
}

fn page_label(page: i32, pages: i32) -> String {
    format!("{page} / {pages}")
}

fn model<T: Clone + 'static>(rows: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(rows))
}
